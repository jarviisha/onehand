use super::*;
use serde_json::json;

fn on(_: &Peer) -> Standing {
    Standing::On
}

fn line(method: &str, params: Value) -> String {
    json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).to_string()
}

/// A connection past `initialize`.
fn ready() -> Conn {
    let mut conn = Conn::default();
    assert!(matches!(
        handle(&mut conn, &line("initialize", json!({})), on),
        Handled::Reply(_)
    ));
    conn
}

fn reply(handled: Handled) -> Value {
    match handled {
        Handled::Reply(line) => serde_json::from_str(&line).unwrap(),
        other => panic!("{other:?}"),
    }
}

fn call(name: &str, arguments: Value) -> Handled {
    handle(
        &mut ready(),
        &line("tools/call", json!({"name": name, "arguments": arguments})),
        on,
    )
}

fn performs(name: &str, arguments: Value) -> Call {
    match call(name, arguments) {
        Handled::Perform { call, .. } => call,
        other => panic!("{name}: {other:?}"),
    }
}

/// The text of a tool's refusal, which must be a tool result marked an error.
fn refusal(handled: Handled) -> String {
    let reply = reply(handled);
    assert_eq!(reply["result"]["isError"], true, "{reply}");
    reply["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .to_string()
}

#[test]
fn every_tool_parses_its_input() {
    assert_eq!(performs("list_projects", json!({})), Call::ListProjects);
    assert_eq!(performs("list_agents", json!({})), Call::ListAgents);
    assert_eq!(performs("list_workflows", json!({})), Call::ListWorkflows);
    assert_eq!(
        performs("list_tasks", json!({})),
        Call::ListTasks { project: None }
    );
    assert_eq!(
        performs("list_tasks", json!({"project": "app"})),
        Call::ListTasks {
            project: Some("app".into())
        }
    );
    assert_eq!(
        performs("stop_task", json!({"task": "t1"})),
        Call::StopTask { task: "t1".into() }
    );
    assert_eq!(
        performs("open_file", json!({"path": "/a/b.rs"})),
        Call::OpenFile {
            path: "/a/b.rs".into()
        }
    );
    assert_eq!(
        performs("open_session", json!({"project": "app", "agent": "Claude"})),
        Call::OpenSession {
            project: "app".into(),
            agent: "Claude".into(),
            prompt: None
        }
    );
    assert_eq!(
        performs(
            "open_session",
            json!({"project": "app", "agent": "Claude", "prompt": "hi"})
        ),
        Call::OpenSession {
            project: "app".into(),
            agent: "Claude".into(),
            prompt: Some("hi".into())
        }
    );
}

#[test]
fn every_tool_refuses_malformed_input() {
    let cases = [
        ("list_tasks", json!({"project": 3}), "project"),
        ("stop_task", json!({}), "task"),
        ("stop_task", json!({"task": ""}), "task"),
        ("open_file", json!({"path": true}), "path"),
        ("open_session", json!({"agent": "Claude"}), "project"),
        ("open_session", json!({"project": "app"}), "agent"),
        (
            "open_session",
            json!({"project": "app", "agent": "Claude", "prompt": 3}),
            "prompt",
        ),
        ("list_agents", json!({"extra": 1}), "extra"),
    ];
    for (name, arguments, named) in cases {
        let why = refusal(call(name, arguments.clone()));
        assert!(why.contains(named), "{name} {arguments}: {why}");
    }
    // Arguments that are not an object at all.
    assert!(refusal(call("list_agents", json!([1]))).contains("object"));
}

#[test]
fn the_catalogue_lists_every_tool_with_a_schema() {
    let reply = reply(handle(&mut ready(), &line("tools/list", json!({})), on));
    let tools = reply["result"]["tools"].as_array().unwrap();
    let names: Vec<_> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(
        names,
        [
            "list_projects",
            "list_agents",
            "open_session",
            "list_workflows",
            "start_workflow",
            "start_issue",
            "list_tasks",
            "stop_task",
            "open_file"
        ]
    );
    for tool in tools {
        assert!(!tool["description"].as_str().unwrap().is_empty());
        assert_eq!(tool["inputSchema"]["type"], "object");
    }
}

#[test]
fn tools_list_is_empty_for_a_session_switched_off() {
    let reply = reply(handle(&mut ready(), &line("tools/list", json!({})), |_| {
        Standing::Off
    }));
    assert_eq!(reply["result"]["tools"], json!([]));
}

#[test]
fn a_call_is_refused_while_switched_off() {
    let handled = handle(
        &mut ready(),
        &line(
            "tools/call",
            json!({"name": "list_agents", "arguments": {}}),
        ),
        |_| Standing::Off,
    );
    assert!(refusal(handled).contains("switched off"));
}

#[test]
fn initialize_without_a_token_is_refused_while_outside_control_is_off() {
    let mut conn = Conn::default();
    let reply = reply(handle(&mut conn, &line("initialize", json!({})), |_| {
        Standing::Refused("outside control is off".into())
    }));
    assert_eq!(reply["error"]["code"], -32001);
    assert_eq!(reply["error"]["message"], "outside control is off");
    // Still nobody: what follows is refused as before an initialize.
    let reply = self::reply(handle(&mut conn, &line("tools/list", json!({})), on));
    assert_eq!(reply["error"]["code"], -32002);
}

#[test]
fn initialize_says_what_it_serves() {
    let mut conn = Conn::default();
    let asked = json!({"protocolVersion": "2025-03-26"});
    let reply = reply(handle(&mut conn, &line("initialize", asked), on));
    let result = &reply["result"];
    assert_eq!(result["protocolVersion"], "2025-03-26");
    assert_eq!(result["capabilities"]["tools"]["listChanged"], true);
    assert_eq!(result["serverInfo"]["name"], "onehand");
}

#[test]
fn initialize_with_a_token_speaks_for_that_session() {
    let token = json!({"_meta": {"onehand": {"session": "abc"}}});
    let mut conn = Conn::default();
    let standing = |peer: &Peer| match peer {
        Peer::Session(token) if token == "abc" => Standing::Off,
        Peer::Session(_) => Standing::Refused("no such session".into()),
        Peer::Outside => Standing::Refused("outside control is off".into()),
    };
    assert!(reply(handle(&mut conn, &line("initialize", token), standing))["result"].is_object());
    assert_eq!(conn.peer(), Some(&Peer::Session("abc".into())));
    // Switched off: connected, and offered nothing.
    let reply = self::reply(handle(&mut conn, &line("tools/list", json!({})), standing));
    assert_eq!(reply["result"]["tools"], json!([]));
    let other = json!({"_meta": {"onehand": {"session": "zzz"}}});
    let reply = self::reply(handle(
        &mut Conn::default(),
        &line("initialize", other),
        standing,
    ));
    assert_eq!(reply["error"]["code"], -32001);
}

#[test]
fn the_proxy_puts_the_token_on_initialize_only() {
    let init = line("initialize", json!({"protocolVersion": "2025-06-18"}));
    let sent: Value = serde_json::from_str(&with_token(&init, "abc")).unwrap();
    assert_eq!(sent["params"]["_meta"]["onehand"]["session"], "abc");
    assert_eq!(sent["params"]["protocolVersion"], "2025-06-18");
    let list = line("tools/list", json!({}));
    assert_eq!(with_token(&list, "abc"), list);
    // A token the client tried to bring is replaced, not trusted.
    let forged = line(
        "initialize",
        json!({"_meta": {"onehand": {"session": "zzz"}}}),
    );
    let sent: Value = serde_json::from_str(&with_token(&forged, "abc")).unwrap();
    assert_eq!(sent["params"]["_meta"]["onehand"]["session"], "abc");
    assert_eq!(with_token("not json", "abc"), "not json");
}

#[test]
fn the_tools_changed_notification_is_a_notification() {
    let note: Value = serde_json::from_str(&tools_changed()).unwrap();
    assert_eq!(note["method"], "notifications/tools/list_changed");
    assert!(note.get("id").is_none());
}

#[test]
fn the_proxy_carries_lines_both_ways() {
    use std::io::{BufRead as _, Write as _};
    let dir = std::env::temp_dir().join(format!("onehand-proxy-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let socket = dir.join("onehand.sock");
    let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
    // The socket's end: echoes each line it reads.
    std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut out = stream.try_clone().unwrap();
        for line in std::io::BufReader::new(stream).lines() {
            writeln!(out, "{}", line.unwrap()).unwrap();
        }
    });
    let input = format!("{}\n", line("initialize", json!({})));
    let mut output = Vec::new();
    proxy(
        &socket,
        "abc",
        std::io::Cursor::new(input.into_bytes()),
        &mut output,
    )
    .unwrap();
    let echoed: Value =
        serde_json::from_slice(output.split(|b| *b == b'\n').next().unwrap()).unwrap();
    assert_eq!(echoed["params"]["_meta"]["onehand"]["session"], "abc");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_malformed_line_is_answered_and_the_connection_goes_on() {
    let mut conn = ready();
    let reply = reply(handle(&mut conn, "{not json", on));
    assert_eq!(reply["error"]["code"], -32700);
    let reply = self::reply(handle(&mut conn, &line("tools/list", json!({})), on));
    assert!(reply["result"]["tools"].is_array());
}

#[test]
fn a_call_before_initialize_is_refused() {
    let handled = handle(
        &mut Conn::default(),
        &line(
            "tools/call",
            json!({"name": "list_agents", "arguments": {}}),
        ),
        on,
    );
    assert_eq!(reply(handled)["error"]["code"], -32002);
}

#[test]
fn a_call_after_outside_control_is_switched_off_is_refused() {
    let mut conn = ready();
    let handled = handle(
        &mut conn,
        &line(
            "tools/call",
            json!({"name": "list_agents", "arguments": {}}),
        ),
        |_| Standing::Refused("outside control is off".into()),
    );
    assert_eq!(reply(handled)["error"]["code"], -32001);
}

#[test]
fn a_notification_is_not_answered() {
    let note = json!({"jsonrpc": "2.0", "method": "notifications/initialized"}).to_string();
    assert!(matches!(handle(&mut ready(), &note, on), Handled::Quiet));
}

#[test]
fn ping_and_unknown_methods_are_answered() {
    let reply = reply(handle(&mut Conn::default(), &line("ping", json!({})), on));
    assert_eq!(reply["result"], json!({}));
    let reply = self::reply(handle(&mut ready(), &line("resources/list", json!({})), on));
    assert_eq!(reply["error"]["code"], -32601);
}

#[test]
fn an_unknown_tool_is_a_protocol_error() {
    let reply = reply(call("answer_permission", json!({})));
    assert_eq!(reply["error"]["code"], -32602);
}

#[test]
fn a_name_two_projects_share_is_refused_naming_both() {
    let projects = vec![
        ("app".to_string(), PathBuf::from("/one/app")),
        ("app".to_string(), PathBuf::from("/two/app")),
    ];
    let why = pick_project(&projects, "app").unwrap_err();
    assert!(
        why.contains("/one/app") && why.contains("/two/app"),
        "{why}"
    );
    assert_eq!(
        pick_project(&projects, "/two/app/").unwrap(),
        Path::new("/two/app")
    );
}

#[test]
fn a_project_is_found_by_path_or_name() {
    let projects = vec![
        ("app".to_string(), PathBuf::from("/one/app")),
        ("site".to_string(), PathBuf::from("/one/site")),
    ];
    assert_eq!(
        pick_project(&projects, "site").unwrap(),
        Path::new("/one/site")
    );
    assert_eq!(
        pick_project(&projects, "/one/app").unwrap(),
        Path::new("/one/app")
    );
    assert!(pick_project(&projects, "nope")
        .unwrap_err()
        .contains("nope"));
}

#[test]
fn a_long_list_says_how_many_it_left_out() {
    let lines: Vec<String> = (0..LISTED + 3).map(|i| i.to_string()).collect();
    let said = listed(lines, "none");
    assert_eq!(said.lines().count(), LISTED + 1);
    assert!(said.ends_with("… and 3 more not listed"), "{said}");
    assert_eq!(listed(Vec::new(), "none"), "none");
}

#[test]
fn answered_marks_a_refusal_as_an_error() {
    let ok: Value = serde_json::from_str(&answered(&json!(4), Ok("done".into()))).unwrap();
    assert_eq!(ok["id"], 4);
    assert_eq!(ok["result"]["isError"], false);
    assert_eq!(ok["result"]["content"][0]["text"], "done");
    let no: Value = serde_json::from_str(&answered(&json!(4), Err("no".into()))).unwrap();
    assert_eq!(no["result"]["isError"], true);
}

#[test]
fn a_batch_or_a_bare_value_is_answered_as_an_invalid_request() {
    for line in [
        "[{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}]",
        "5",
        "\"x\"",
    ] {
        let reply = reply(handle(&mut ready(), line, on));
        assert_eq!(reply["error"]["code"], -32600, "{line}");
        assert_eq!(reply["id"], Value::Null);
    }
    let no_method = json!({"jsonrpc": "2.0", "id": 9}).to_string();
    let reply = reply(handle(&mut ready(), &no_method, on));
    assert_eq!(reply["error"]["code"], -32600);
    assert_eq!(reply["id"], 9);
}

#[test]
fn the_starts_parse_their_input() {
    assert_eq!(
        performs(
            "start_workflow",
            json!({"project": "app", "workflow": "fix", "title": "Fix it"})
        ),
        Call::StartWorkflow {
            project: "app".into(),
            workflow: "fix".into(),
            title: "Fix it".into(),
            brief: None
        }
    );
    assert_eq!(
        performs(
            "start_workflow",
            json!({"project": "app", "workflow": "fix", "title": "Fix it", "brief": "how"})
        ),
        Call::StartWorkflow {
            project: "app".into(),
            workflow: "fix".into(),
            title: "Fix it".into(),
            brief: Some("how".into())
        }
    );
    assert_eq!(
        performs("start_issue", json!({"project": "app", "issue": 12})),
        Call::StartIssue {
            project: "app".into(),
            issue: 12,
            workflow: None
        }
    );
    assert_eq!(
        performs(
            "start_issue",
            json!({"project": "app", "issue": 12, "workflow": "fix"})
        ),
        Call::StartIssue {
            project: "app".into(),
            issue: 12,
            workflow: Some("fix".into())
        }
    );
}

#[test]
fn the_starts_refuse_malformed_input() {
    let cases = [
        (
            "start_workflow",
            json!({"project": "app", "workflow": "fix"}),
            "title",
        ),
        (
            "start_workflow",
            json!({"project": "app", "title": "t"}),
            "workflow",
        ),
        ("start_issue", json!({"project": "app"}), "issue"),
        (
            "start_issue",
            json!({"project": "app", "issue": "12"}),
            "issue",
        ),
        (
            "start_issue",
            json!({"project": "app", "issue": 0}),
            "issue",
        ),
        (
            "start_issue",
            json!({"project": "app", "issue": -1}),
            "issue",
        ),
        (
            "start_issue",
            json!({"project": "app", "issue": 1.5}),
            "issue",
        ),
    ];
    for (name, arguments, named) in cases {
        let why = refusal(call(name, arguments.clone()));
        assert!(why.contains(named), "{name} {arguments}: {why}");
    }
}

#[test]
fn the_proxy_entry_starts_onehand_as_its_own_proxy() {
    let entry = proxy_entry(
        Path::new("/bin/onehand"),
        Path::new("/c/onehand.sock"),
        "abc",
    );
    assert_eq!(entry["name"], "onehand");
    assert_eq!(entry["command"], "/bin/onehand");
    assert_eq!(
        entry["args"],
        json!(["--mcp-proxy", "/c/onehand.sock", "abc"])
    );
    assert_eq!(entry["env"], json!([]));
}

#[test]
fn every_token_is_its_own() {
    let (a, b) = (new_token(), new_token());
    assert_ne!(a, b);
    assert_eq!(a.len(), 32);
    assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
}
