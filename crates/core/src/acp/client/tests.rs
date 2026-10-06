use super::{
    base64_encode, connect_over, elicit_result, explain, file_uri, parse_elicitation, LastWords,
    Transport, STDERR_LINE_CAP, STDERR_TAIL_LINES,
};
use crate::acp::{AcpEvent, AcpRequest, ElicitKind, ElicitOutcome, ElicitValue};
use futures::StreamExt as _;
use serde_json::{json, Value};
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};

/// The agent's side of a scripted transport: read what the client wrote,
/// write back what an adapter would have said.
struct Agent {
    lines: tokio::io::Lines<BufReader<tokio::io::ReadHalf<tokio::io::DuplexStream>>>,
    out: tokio::io::WriteHalf<tokio::io::DuplexStream>,
}

impl Agent {
    /// The next JSON-RPC message the client sent.
    async fn heard(&mut self) -> Value {
        let line = self
            .lines
            .next_line()
            .await
            .expect("read the client's pipe")
            .expect("the client wrote a line");
        serde_json::from_str(&line).expect("the client writes JSON")
    }

    /// Say one message to the client.
    async fn say(&mut self, msg: Value) {
        let mut line = serde_json::to_string(&msg).unwrap();
        line.push('\n');
        self.out.write_all(line.as_bytes()).await.unwrap();
    }

    /// Answer the handshake the way an adapter would, and hand back the
    /// session id it minted.
    ///
    /// Every test past the first one starts here, because nothing the client
    /// does is reachable until it is ready.
    async fn handshake(&mut self) -> String {
        let hello = self.heard().await;
        assert_eq!(hello["method"], "initialize");
        self.say(json!({
            "jsonrpc": "2.0",
            "id": hello["id"],
            "result": { "protocolVersion": 1, "agentCapabilities": {} }
        }))
        .await;

        let new = self.heard().await;
        assert_eq!(new["method"], "session/new");
        self.say(json!({
            "jsonrpc": "2.0",
            "id": new["id"],
            "result": { "sessionId": "s-test" }
        }))
        .await;
        "s-test".to_string()
    }
}

/// Run the client over a scripted transport, with its events collected on a
/// task of their own.
///
/// The serve loop is folded into the stream, so something has to poll it for
/// the client to make any progress at all — driving it here is what lets the
/// test body read and write the agent's side as a conversation.
fn client() -> (Agent, tokio::sync::mpsc::UnboundedReceiver<AcpEvent>) {
    let (transport, agent) = Transport::scripted();
    let (read, out) = tokio::io::split(agent);
    let (tx, events) = tokio::sync::mpsc::unbounded_channel();
    tokio::spawn(async move {
        let stream = connect_over(transport, PathBuf::from("/tmp"), None);
        futures::pin_mut!(stream);
        while let Some(event) = stream.next().await {
            if tx.send(event).is_err() {
                break;
            }
        }
    });
    (
        Agent {
            lines: BufReader::new(read).lines(),
            out,
        },
        events,
    )
}

/// **The capability that makes multiple-choice prompts appear at all.** The
/// adapter puts `AskUserQuestion` in `disallowedTools` unless the client
/// advertises it, so dropping it does not break anything visibly — the model
/// simply stops asking and starts guessing, which is a wrong answer wearing
/// the shape of a right one.
#[tokio::test]
async fn the_handshake_advertises_the_form_capability_questions_need() {
    let (mut agent, _events) = client();

    let hello = agent.heard().await;

    assert_eq!(hello["method"], "initialize");
    // Present, whatever it holds. The capability *is* the key: the value is
    // an empty object today, and an adapter reads this the same way — the
    // question it asks is whether the client claimed it can render a form.
    assert!(
        !hello["params"]["clientCapabilities"]["elicitation"]["form"].is_null(),
        "{hello:#}"
    );
}

/// Wait until the client reports itself connected, and take the request
/// channel it hands over.
///
/// Waited *for* rather than counted to: the events before it are whatever
/// the adapter's answer happened to carry — a session id always, modes and
/// config options if it offered any — and a test that counted would break on
/// a reply that mentioned one more of them.
async fn connected(
    events: &mut tokio::sync::mpsc::UnboundedReceiver<AcpEvent>,
) -> crate::acp::types::ReqTx {
    while let Some(event) = events.recv().await {
        if let AcpEvent::Connected { tx, .. } = event {
            return tx;
        }
    }
    panic!("the client never reported itself connected");
}

/// Wait for the next permission the client parks.
async fn parked(
    events: &mut tokio::sync::mpsc::UnboundedReceiver<AcpEvent>,
) -> crate::acp::PermissionRequest {
    while let Some(event) = events.recv().await {
        if let AcpEvent::Permission(request) = event {
            return request;
        }
    }
    panic!("the permission never reached the caller");
}

/// **A parked permission is the one reverse request the client answers late,
/// and nothing has ever checked that the answer arrives.** No response is
/// written when it comes in — the agent's tool call blocks on the user — so
/// dropping the round trip does not fail anything, it hangs the turn.
///
/// The response has to name the *same* rpc id it was asked under: an answer
/// carrying the wrong one settles some other request, or none.
#[tokio::test]
async fn a_parked_permission_is_answered_under_the_id_it_was_asked_on() {
    let (mut agent, mut events) = client();
    agent.handshake().await;
    let tx = connected(&mut events).await;

    agent
        .say(json!({
            "jsonrpc": "2.0",
            "id": 77,
            "method": "session/request_permission",
            "params": {
                "sessionId": "s-test",
                "toolCall": { "toolCallId": "tc1", "title": "Run cargo test" },
                "options": [
                    { "optionId": "allow", "name": "Allow", "kind": "allow_once" },
                    { "optionId": "deny", "name": "Deny", "kind": "reject_once" }
                ]
            }
        }))
        .await;

    let request = parked(&mut events).await;
    assert_eq!(request.title, "Run cargo test");
    assert_eq!(request.options.len(), 2);

    tx.send(AcpRequest::PermissionResponse {
        rpc_id: request.rpc_id.clone(),
        option_id: Some("allow".to_string()),
    })
    .expect("the client is still serving");

    let answer = agent.heard().await;
    assert_eq!(answer["id"], json!(77), "{answer:#}");
    assert_eq!(answer["result"]["outcome"]["outcome"], "selected");
    assert_eq!(answer["result"]["outcome"]["optionId"], "allow");
}

/// An adapter that goes away has to surface, or the session sits there
/// looking alive while nothing is behind it.
///
/// The sentence is deliberately not pinned. Which side notices first — a
/// write that finds nobody, or a read that ends — is a fact about pipes and
/// scheduling rather than about this loop, and asserting one of them would
/// be asserting the operating system. What the caller is owed is that the
/// stream ends, and ends by saying so.
#[tokio::test]
async fn an_adapter_that_goes_away_ends_the_stream_rather_than_hanging() {
    let (agent, mut events) = client();
    drop(agent);

    let mut last = None;
    while let Some(event) = events.recv().await {
        last = Some(event);
    }

    assert!(matches!(&last, Some(AcpEvent::Disconnected(_))), "{last:?}");
}

/// **An adapter that accepts the connection and then says nothing is the
/// worst of the failures**, because every other one ends a stream somebody
/// is reading: this one leaves a session that looks like it is starting up,
/// for as long as anybody is willing to wait.
///
/// Time is paused, so the ninety seconds this waits are not spent. Left
/// running, this single test costs more than the rest of the suite together
/// — which is how it was found.
#[tokio::test(start_paused = true)]
async fn an_adapter_that_never_answers_the_handshake_gives_up_on_its_own() {
    let (mut agent, mut events) = client();
    let hello = agent.heard().await;
    assert_eq!(hello["method"], "initialize", "{hello:#}");

    let mut last = None;
    while let Some(event) = events.recv().await {
        last = Some(event);
    }

    assert!(
        matches!(&last, Some(AcpEvent::Disconnected(why)) if why == "handshake timed out"),
        "{last:?}"
    );
}

/// **A form the client cannot draw is declined where it arrives.** Parking
/// it would block the agent's tool call on a card nobody can ever fill in,
/// and the turn would stand still with nothing on screen to say why. The
/// caller is told nothing, because there is nothing it could do.
#[tokio::test]
async fn a_form_that_cannot_be_drawn_is_declined_instead_of_parked() {
    let (mut agent, mut events) = client();
    agent.handshake().await;
    connected(&mut events).await;

    agent
        .say(json!({
            "jsonrpc": "2.0",
            "id": 31,
            "method": "elicitation/create",
            "params": { "mode": "url", "sessionId": "s-test", "url": "https://example.invalid" }
        }))
        .await;

    let answer = agent.heard().await;
    assert_eq!(answer["id"], json!(31), "{answer:#}");
    assert_eq!(answer["result"]["action"], "decline");
    assert!(
        !matches!(events.try_recv(), Ok(AcpEvent::Elicitation(_))),
        "a card nobody can fill in was put in front of the user"
    );
}

/// The shape Claude's adapter sends for a two-question `AskUserQuestion`:
/// indexed fields, each with its own `_custom` "Other" box.
fn ask_params() -> serde_json::Value {
    json!({
        "mode": "form",
        "sessionId": "s1",
        "toolCallId": "tool-7",
        "message": "Please answer the following questions.",
        "requestedSchema": {
            "type": "object",
            "properties": {
                "question_0": {
                    "type": "string",
                    "title": "Storage",
                    "description": "Where should it live?",
                    "oneOf": [
                        { "const": "sqlite", "title": "SQLite", "description": "One file" },
                        { "const": "postgres", "title": "Postgres" },
                    ],
                },
                "question_0_custom": { "type": "string", "title": "Other" },
                "question_1": {
                    "type": "array",
                    "title": "Extras",
                    "items": { "anyOf": [{ "const": "tests" }, { "const": "docs" }] },
                },
                "question_1_custom": { "type": "string", "title": "Other" },
            },
        },
    })
}

#[test]
fn parses_ask_user_question_form() {
    let e = parse_elicitation(json!(3), &ask_params()).unwrap();
    assert_eq!(e.tool_call_id.as_deref(), Some("tool-7"));
    // The `_custom` boxes attach to their question instead of standing as
    // fields of their own.
    assert_eq!(e.fields.len(), 2);

    let q0 = &e.fields[0];
    assert_eq!(q0.key, "question_0");
    assert_eq!(q0.custom_key.as_deref(), Some("question_0_custom"));
    assert_eq!(q0.description.as_deref(), Some("Where should it live?"));
    let ElicitKind::Select(choices) = &q0.kind else {
        panic!("expected a select")
    };
    assert_eq!(choices[0].value, "sqlite");
    assert_eq!(choices[0].label, "SQLite");
    assert_eq!(choices[0].description.as_deref(), Some("One file"));
    // No `title` → the label falls back to the wire value.
    assert_eq!(choices[1].label, "Postgres");

    let ElicitKind::MultiSelect(extras) = &e.fields[1].kind else {
        panic!("expected multi")
    };
    assert_eq!(extras.len(), 2);
}

#[test]
fn orders_questions_numerically_not_lexically() {
    // `serde_json` maps are sorted, so "question_10" sorts before
    // "question_2" — the numeric suffix has to drive the order.
    let mut props = serde_json::Map::new();
    for n in [0u32, 2, 10] {
        props.insert(format!("question_{n}"), json!({ "type": "string" }));
    }
    let params = json!({
        "message": "m",
        "requestedSchema": { "type": "object", "properties": props },
    });
    let e = parse_elicitation(json!(1), &params).unwrap();
    let keys: Vec<&str> = e.fields.iter().map(|f| f.key.as_str()).collect();
    assert_eq!(keys, ["question_0", "question_2", "question_10"]);
}

#[test]
fn parses_plain_enum_and_free_text_fields() {
    // A generic form (an MCP server's, or the refusal-fallback prompt):
    // arbitrary keys, MCP-style `enum` + `enumNames`, and a bare string.
    let params = json!({
        "message": "Retry?",
        "requestedSchema": { "type": "object", "properties": {
            "choice": { "type": "string", "enum": ["retry", "keep"],
                        "enumNames": ["Retry", "Keep the refusal"] },
            "note": { "type": "string", "title": "Note" },
        }},
    });
    let e = parse_elicitation(json!(1), &params).unwrap();
    let ElicitKind::Select(c) = &e.fields[0].kind else {
        panic!("expected a select")
    };
    assert_eq!(c[0].label, "Retry");
    assert_eq!(c[1].value, "keep");
    assert!(e.fields[0].custom_key.is_none()); // no `choice_custom` property
    assert_eq!(e.fields[1].kind, ElicitKind::Text);
}

#[test]
fn unrenderable_forms_are_rejected_so_the_caller_declines() {
    let url = json!({ "mode": "url", "url": "https://example.com" });
    assert!(parse_elicitation(json!(1), &url).is_none());
    // No schema at all, and a schema with no properties.
    assert!(parse_elicitation(json!(1), &json!({ "message": "hi" })).is_none());
    let empty = json!({ "requestedSchema": { "type": "object", "properties": {} } });
    assert!(parse_elicitation(json!(1), &empty).is_none());
}

#[test]
fn accept_result_carries_answers_by_key() {
    let out = elicit_result(ElicitOutcome::Accept(vec![
        ("question_0".into(), ElicitValue::Text("sqlite".into())),
        (
            "question_1".into(),
            ElicitValue::List(vec!["tests".into(), "docs".into()]),
        ),
    ]));
    assert_eq!(
        out,
        json!({ "action": "accept",
                "content": { "question_0": "sqlite", "question_1": ["tests", "docs"] } })
    );
    assert_eq!(
        elicit_result(ElicitOutcome::Decline),
        json!({ "action": "decline" })
    );
    assert_eq!(
        elicit_result(ElicitOutcome::Cancel),
        json!({ "action": "cancel" })
    );
}

#[test]
fn base64_matches_known_vectors() {
    assert_eq!(base64_encode(b""), "");
    assert_eq!(base64_encode(b"f"), "Zg==");
    assert_eq!(base64_encode(b"fo"), "Zm8=");
    assert_eq!(base64_encode(b"foo"), "Zm9v");
    assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
}

#[test]
fn file_uri_percent_encodes_reserved_and_non_ascii() {
    use std::path::Path;
    assert_eq!(
        file_uri(Path::new("/home/u/proj/src/lib.rs")),
        "file:///home/u/proj/src/lib.rs"
    );
    assert_eq!(
        file_uri(Path::new("/home/u/My Docs/notes#1.txt")),
        "file:///home/u/My%20Docs/notes%231.txt"
    );
    // Multibyte (Vietnamese) escapes per UTF-8 byte.
    assert_eq!(
        file_uri(Path::new("/a/tệp.txt")),
        "file:///a/t%E1%BB%87p.txt"
    );
}

/// The tail keeps the *last* lines, because a tool says what went wrong
/// after it has finished trying — an oldest-first buffer would fill with
/// the banner and drop the reason.
#[test]
fn the_stderr_tail_keeps_the_last_lines_and_drops_the_blanks() {
    let mut tail = LastWords::default();
    tail.remember("first");
    for line in ["", "   "] {
        tail.remember(line);
    }
    for n in 0..STDERR_TAIL_LINES {
        tail.remember(&format!("line {n}"));
    }

    assert_eq!(tail.kept.len(), STDERR_TAIL_LINES);
    assert!(!tail.kept.contains(&"first".to_string()), "{:?}", tail.kept);
    assert_eq!(tail.kept.back().map(String::as_str), Some("line 19"));
    assert!(tail.kept.iter().all(|l| !l.trim().is_empty()));
    // The blanks were skipped, not dropped: only `first` fell off.
    assert_eq!(tail.dropped, 1);
}

/// The npm block this was written for is five lines and arrives behind
/// whatever the tool printed first. The tail has to hold the block *and*
/// its preamble, or the line naming the fault class — which comes first —
/// is the one evicted.
#[test]
fn a_real_npm_error_block_survives_the_noise_printed_ahead_of_it() {
    let mut tail = LastWords::default();
    for line in ["npm warn exec", "npm warn deprecated something@1.0.0"] {
        tail.remember(line);
    }
    for line in [
        "npm error code ETARGET",
        "npm error notarget No matching version found for zod@4.6.5.",
        "npm error notarget In most cases you or one of your dependencies",
        "npm error notarget are requesting a package version that doesn't exist.",
        "npm error A complete log of this run can be found in: /tmp/x.log",
    ] {
        tail.remember(line);
    }

    let said = tail.say().expect("it said something");
    assert!(said.contains("npm error code ETARGET"), "{said}");
    assert!(
        said.contains("No matching version found for zod@4.6.5"),
        "{said}"
    );
    assert_eq!(tail.dropped, 0);
}

/// A line is cut on a character boundary, not a byte one — an adapter is
/// free to print a path, and half a multi-byte character would panic the
/// slice rather than shorten the line.
#[test]
fn a_long_stderr_line_is_cut_without_splitting_a_character() {
    let mut tail = LastWords::default();
    tail.remember(&"đường".repeat(200));

    let only = tail.kept.front().expect("one line");
    assert_eq!(only.chars().count(), STDERR_LINE_CAP);
}

/// A tail that dropped lines says so. Without the count the survivors read
/// as everything the adapter said, which is the bound lying rather than
/// binding.
#[test]
fn a_tail_that_lost_lines_says_how_many() {
    let mut tail = LastWords::default();
    for n in 0..STDERR_TAIL_LINES + 3 {
        tail.remember(&format!("line {n}"));
    }

    let said = tail.say().expect("it said something");
    assert!(said.starts_with("(+3 earlier lines) · "), "{said}");
}

/// The whole point, end to end, against a real child: a process that says
/// why on stderr and exits without ever speaking protocol must reach the
/// reader with the reason attached, not as the bare protocol sentence.
///
/// Repeated, because the failure this guards is a race rather than a
/// mistake — the tail is filled by a task the serve loop does not wait for,
/// so a single pass can pick the scheduling that happens to work.
///
/// **Multi-threaded on purpose.** The bridge that drives this stream in the
/// app builds a multi-threaded runtime, and the default test flavour is a
/// single thread — which is the one arrangement where the drain and the
/// serve loop cannot truly run at once.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_child_that_dies_before_speaking_carries_its_stderr_into_the_failure() {
    for attempt in 0..20 {
        let mut events = Box::pin(super::connect(
            "sh".into(),
            vec![
                "-c".into(),
                // stdout is closed *first* and the reason written a beat
                // later, so the serve loop has already seen EOF while the
                // child is still alive with something left to say. The
                // pause makes that ordering certain rather than a matter of
                // scheduling: without it the test passed on a fast machine
                // and failed on a loaded one.
                "exec 1>&-; sleep 0.1; echo 'npm error code ETARGET' >&2; exit 1".into(),
            ],
            crate::agent::AgentAuth::Inherit,
            std::env::temp_dir(),
            None,
        ));
        let mut last = None;
        while let Some(event) = events.next().await {
            last = Some(event);
        }
        match last {
            Some(AcpEvent::Disconnected(why)) => {
                assert!(why.contains("ETARGET"), "attempt {attempt}: {why}")
            }
            other => panic!("attempt {attempt}: {other:?}"),
        }
    }
}

/// A line that is not UTF-8 must not end the drain. Read as text, the first
/// such line is an error the loop stops on, and from then on nothing
/// empties the pipe — so the reason printed after it is lost, and an
/// adapter that keeps writing blocks once the pipe fills.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_line_that_is_not_utf8_does_not_stop_the_drain() {
    let mut events = Box::pin(super::connect(
        "sh".into(),
        vec![
            "-c".into(),
            r"printf '\377\376 garbage\n' >&2; echo 'npm error code ETARGET' >&2; exit 1".into(),
        ],
        crate::agent::AgentAuth::Inherit,
        std::env::temp_dir(),
        None,
    ));
    let mut last = None;
    while let Some(event) = events.next().await {
        last = Some(event);
    }
    match last {
        Some(AcpEvent::Disconnected(why)) => assert!(why.contains("ETARGET"), "{why}"),
        other => panic!("{other:?}"),
    }
}

/// What the protocol saw stays in front; the adapter's own words follow it.
/// The empty case must not leave a dangling separator, since that reads as
/// a message the app failed to fill in.
#[test]
fn a_failure_carries_the_adapters_last_words_when_it_said_any() {
    let tail: super::StderrTail = Default::default();
    assert_eq!(
        explain("adapter closed stdout".into(), &tail),
        "adapter closed stdout"
    );

    for line in ["npm error code ETARGET", "npm error No matching version"] {
        tail.lock().expect("fresh lock").remember(line);
    }
    assert_eq!(
        explain("adapter closed stdout".into(), &tail),
        "adapter closed stdout: npm error code ETARGET · npm error No matching version"
    );
}
