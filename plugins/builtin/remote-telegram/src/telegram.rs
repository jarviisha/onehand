//! The Telegram Bot API as a [`RemoteChannel`].
//!
//! A long poll out and two calls back, over HTTPS. The shape is the ACP client's
//! and for the same reason: the serve loop is folded into the stream returned by
//! [`RemoteChannel::connect`], so the bridge advances only while something polls
//! it, and dropping the stream ends the poll. There is no stop to remember.
//!
//! **Losing the connection is the normal condition, not the failure.** A long
//! poll is a request deliberately left open for half a minute, so it is cut by
//! every sleeping laptop, every changed network and every restart on the far
//! side. Those are retried here with a widening gap, and nothing is reported
//! upward — a bridge that surfaced each of them would spend its life announcing
//! that it is still working. **The handshake waits the same way**, because the
//! app runs it once at startup and a machine whose wifi is not up yet would
//! otherwise have no bridge until somebody restarted the app. Only a refusal
//! that retrying cannot fix ends the stream with [`RemoteEvent::Disconnected`]:
//! a token the far side will not accept, or another process already polling this
//! same bot.
//!
//! **Nothing this module says about a failure carries the token.** The Bot API
//! puts it in the URL path and an HTTP client names the URL in its own error
//! text, so a dropped connection — the most ordinary thing that happens to a
//! long poll — would print a working credential to stderr. [`Telegram::redact`]
//! is what stands between those two facts, and every failure is worded through
//! the one function that holds the token.

use futures::channel::mpsc::Sender as EventTx;
use futures::stream::{self, Stream};
use futures::{SinkExt as _, StreamExt as _};
use onehand_core::remote::types::{
    Button, Outbound, RemoteChannel, RemoteEvent, RemoteRequest, ReqRx,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::time::Duration;

/// The Bot API's root. A field on [`Telegram`] rather than a constant reached
/// for directly, so a test can point the client somewhere that is not the
/// internet.
const API_ROOT: &str = "https://api.telegram.org";

/// How many events may sit unread before the poll loop back-pressures. The same
/// buffer the ACP stream uses: a burst that out-runs the app slows the poll
/// rather than growing without bound.
const EVENT_BUFFER: usize = 32;

/// How long the far side holds a poll open with nothing to say.
///
/// Long enough that an idle bridge makes a couple of requests a minute rather
/// than one a second, and short enough to stay well inside the client's own
/// timeout so a healthy poll is never mistaken for a stalled one.
const POLL_SECS: u64 = 25;

/// The client's ceiling on a single request, comfortably above [`POLL_SECS`] so
/// only a poll that has actually stopped answering trips it.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(POLL_SECS + 15);

/// The first pause after a failed poll, and the longest it grows to.
///
/// It doubles in between. The ceiling is a minute because the thing being waited
/// for is usually a network coming back, and a bridge that had backed off to an
/// hour would still be asleep long after the laptop woke up.
const BACKOFF_MIN: Duration = Duration::from_secs(1);
const BACKOFF_MAX: Duration = Duration::from_secs(60);

/// How many times one outgoing message is retried after the far side asks it to
/// slow down.
///
/// One. The API answers a rate limit with how long to wait, so the retry is a
/// wait and not a guess — and a notification worth more than two attempts is a
/// notification the app should not be trying to force through, because by the
/// time a third would land the thing it announced has usually been answered in
/// the window instead.
const RATE_LIMIT_RETRIES: usize = 1;

/// The longest pause this will honour when told to slow down.
///
/// A far side asking for ten minutes is asking this bridge to stop sending and
/// hold the whole queue behind one message, so the message is dropped instead.
const RATE_LIMIT_MAX_WAIT: Duration = Duration::from_secs(30);

/// The Telegram bridge, ready to be connected.
pub struct Telegram {
    token: String,
    root: String,
}

impl Telegram {
    pub fn new(token: String) -> Self {
        Self {
            token,
            root: API_ROOT.to_string(),
        }
    }

    /// Point this channel at another API root. Test seam.
    #[cfg(test)]
    fn with_root(mut self, root: impl Into<String>) -> Self {
        self.root = root.into();
        self
    }

    /// The URL one method is called at.
    ///
    /// **The token is in the path**, which is the whole reason [`Self::redact`]
    /// exists: this string must never reach a log, a message or an event.
    fn url(&self, method: &str) -> String {
        format!("{}/bot{}/{method}", self.root, self.token)
    }

    /// `text` with the credential taken out of it.
    ///
    /// **Every word this module says about a failure goes through here**, and it
    /// is not belt-and-braces. This API puts the bot token in the *path*, and an
    /// HTTP client's own error text names the URL it was talking to — so an
    /// ordinary dropped connection, the most routine thing that happens to a
    /// long poll, prints a working bearer credential to stderr. It would defeat
    /// the whole point of keeping that credential out of the config file, and it
    /// would do it on the first bad minute of wifi.
    ///
    /// A blank token is left alone rather than replaced: substituting the empty
    /// string matches between every character and would turn a one-line message
    /// into a wall of markers. Nothing gets that far — a blank token is refused
    /// before a channel is built — but the guard costs one comparison and the
    /// failure it prevents is silent.
    fn redact(&self, text: String) -> String {
        if self.token.is_empty() {
            return text;
        }
        text.replace(&self.token, "<token>")
    }
}

impl RemoteChannel for Telegram {
    fn name(&self) -> &'static str {
        "Telegram"
    }

    fn connect(
        self: Box<Self>,
        requests: ReqRx,
    ) -> std::pin::Pin<Box<dyn Stream<Item = RemoteEvent> + Send>> {
        let (sender, receiver) = futures::channel::mpsc::channel(EVENT_BUFFER);

        // The loop is the stream, not a task beside it: the channel only
        // advances while the app polls, and dropping the stream is what shuts
        // the poll down. `runner` never yields an item -- it exists to be
        // driven, and combining it with the event channel this way is what keeps
        // `connect` a plain stream rather than a handle pair someone has to
        // remember to pump.
        let runner = stream::once(async move {
            let mut out = sender;
            if let Err(reason) = run(*self, requests, &mut out).await {
                let _ = out.send(RemoteEvent::Disconnected(reason)).await;
            }
        })
        .filter_map(|()| async { None });

        Box::pin(stream::select(receiver, runner))
    }
}

/// Put a cryptography provider in rustls' process-wide slot.
///
/// The TLS client this crate builds is compiled without one chosen for it, so
/// the choice has to be made here, once, before the first connection. The result
/// is ignored on purpose: an error means some other part of the process
/// installed a provider first, and a bridge that refused to run because
/// somebody else had already answered the same question would be failing over
/// nothing.
fn install_crypto() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

/// Connect, then run the poll and the send loop side by side until either ends.
async fn run(bot: Telegram, requests: ReqRx, out: &mut EventTx<RemoteEvent>) -> Result<(), String> {
    install_crypto();
    let client = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|e| format!("could not start an HTTPS client: {e}"))?;

    // The handshake, and the one call whose refusal is worth ending on: a token
    // the far side rejects is rejected identically every time, so retrying it is
    // a loop that never terminates and never says why.
    let name = greet(&client, &bot).await?;
    out.send(RemoteEvent::Connected { name })
        .await
        .map_err(|_| "nobody is listening to the bridge".to_string())?;

    // Two loops in one future rather than two tasks. Selecting between them
    // means neither is cancelled while it is mid-flight -- `select!` only drops
    // the other branch once one has *finished*, and both of these run until the
    // bridge is over -- and it keeps everything owned by the stream, so dropping
    // the stream still ends both.
    let poller = poll_forever(&client, &bot, out);
    let sender = send_forever(&client, &bot, requests);
    tokio::pin!(poller, sender);
    tokio::select! {
        reason = &mut poller => reason,
        () = &mut sender => Ok(()),
    }
}

/// `getMe`, for the bot's own name and to find out whether the token works.
///
/// **It waits rather than gives up**, with the same widening gap the poll uses,
/// and only a refusal about the credential ends it. The handshake is the first
/// thing this module does and the app runs it once, at startup — so treating a
/// dead network as terminal means an app launched before wifi is up, or behind a
/// VPN that has not connected, or on a captive portal, has no bridge at all
/// until somebody notices and restarts it. That is the machine deciding it is
/// offline for the day because of the second it was asked.
async fn greet(client: &reqwest::Client, bot: &Telegram) -> Result<String, String> {
    let mut backoff = BACKOFF_MIN;
    loop {
        match call::<Me>(client, bot, "getMe", &json!({}))
            .await
            .and_then(ApiReply::into_result)
        {
            Ok(me) => {
                return Ok(me
                    .username
                    .map(|name| format!("@{name}"))
                    .unwrap_or(me.first_name));
            }
            // The one answer worth ending on: a token the far side rejects is
            // rejected identically every time, so retrying it is a loop that
            // never terminates and never says why.
            Err(Trouble::Fatal(why)) => {
                return Err(format!("Telegram refused the token: {why}"));
            }
            Err(trouble) => {
                eprintln!(
                    "onehand: cannot reach Telegram yet ({}); retrying in {backoff:?}",
                    trouble.reason()
                );
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(BACKOFF_MAX);
            }
        }
    }
}

/// Poll until the token stops working, widening the gap after every failure.
async fn poll_forever(
    client: &reqwest::Client,
    bot: &Telegram,
    out: &mut EventTx<RemoteEvent>,
) -> Result<(), String> {
    // The far side replays an update until it is acknowledged, and the
    // acknowledgement is the offset of the *next* one. Advanced only after a
    // batch has been handed on, so a poll that is cancelled or fails re-delivers
    // rather than losing what it was carrying.
    //
    // It starts at -1, which asks for the last update and nothing before it.
    // **The backlog is deliberately thrown away.** The far side holds unread
    // updates for a day, so starting from the beginning would mean an app opened
    // on Monday running a prompt somebody sent on Sunday and has long since
    // stopped expecting -- and answering a permission that belongs to an agent
    // which no longer exists. Nothing that arrived while the app was shut is
    // acted on; the bridge starts from now.
    let mut offset: i64 = -1;
    let mut primed = false;
    let mut backoff = BACKOFF_MIN;

    loop {
        let body = json!({
            "offset": offset,
            // The first call is the one that finds out where "now" is, so it
            // asks rather than waits.
            "timeout": if primed { POLL_SECS } else { 0 },
            // Everything else the API can send is noise this bridge has no
            // reading for, and asking for it costs a round trip per edit,
            // reaction and membership change in every group the bot is in.
            "allowed_updates": ["message", "callback_query"],
        });

        let updates: Vec<Update> = match call::<Vec<Update>>(client, bot, "getUpdates", &body)
            .await
            .and_then(ApiReply::into_result)
        {
            Ok(updates) => updates,
            Err(Trouble::Fatal(why)) => return Err(why),
            Err(Trouble::Transient(why)) => {
                eprintln!("onehand: Telegram poll failed ({why}); retrying in {backoff:?}");
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(BACKOFF_MAX);
                continue;
            }
            Err(Trouble::SlowDown(_, wait)) => {
                tokio::time::sleep(wait.min(BACKOFF_MAX)).await;
                continue;
            }
        };

        backoff = BACKOFF_MIN;
        for update in updates {
            offset = offset.max(update.update_id + 1);
            let Some(event) = update.into_event() else {
                continue;
            };
            // Whatever the priming call turned up is the backlog, by
            // definition: it was sent before this bridge was listening.
            if !primed {
                continue;
            }
            if out.send(event).await.is_err() {
                // Nobody is listening any more, which is how the app says it is
                // done with the bridge.
                return Ok(());
            }
        }
        // An empty queue leaves the offset where it started, and asking for the
        // last update for ever would drop all but one of any pair that arrived
        // inside a single poll. Nothing is being skipped by moving to zero here:
        // zero means everything unacknowledged, and the queue just answered that
        // it holds nothing.
        if !primed {
            primed = true;
            offset = offset.max(0);
        }
    }
}

/// Carry out what the app asks, until it stops asking.
async fn send_forever(client: &reqwest::Client, bot: &Telegram, mut requests: ReqRx) {
    while let Some(request) = requests.recv().await {
        let (method, body) = match request {
            RemoteRequest::Send(out) => ("sendMessage", send_body(&out)),
            RemoteRequest::Ack { press_id, text } => (
                "answerCallbackQuery",
                json!({
                    "callback_query_id": press_id,
                    "text": text.map(|t| clip(&t, ACK_TEXT_MAX)),
                }),
            ),
        };
        deliver(client, bot, method, &body).await;
    }
}

/// One outgoing call, with the far side's own answer to being rate limited.
///
/// A failure is written to stderr and nothing more. What is being sent is a copy
/// of something the app is already showing in its own window, so a message that
/// does not land costs a notification rather than the thing it was about — and
/// there is nowhere better to report it to, since the channel that would carry
/// the report is the one that just failed.
async fn deliver(client: &reqwest::Client, bot: &Telegram, method: &str, body: &Value) {
    for attempt in 0..=RATE_LIMIT_RETRIES {
        match call::<Value>(client, bot, method, body)
            .await
            .and_then(ApiReply::into_result)
        {
            Ok(_) => return,
            // The wait is only worth taking while there is an attempt left to
            // take it for; on the last one it falls through and is reported,
            // rather than sleeping and then giving up in silence.
            Err(Trouble::SlowDown(why, wait))
                if attempt < RATE_LIMIT_RETRIES && wait <= RATE_LIMIT_MAX_WAIT =>
            {
                eprintln!("onehand: Telegram asked to slow down ({why}); waiting {wait:?}");
                tokio::time::sleep(wait).await;
            }
            Err(t) => {
                eprintln!("onehand: could not send to Telegram: {}", t.reason());
                return;
            }
        }
    }
}

/// What the API will accept in one message, and in the little toast that
/// answers a press.
///
/// **Over the limit is a refusal, not a truncation** — the whole call fails and
/// the message simply never arrives, which for a notification means silence with
/// nothing to say why. So the clipping happens here, where the limits are, and
/// what is clipped is at least still delivered. Both are a shade under the
/// documented figure, so the ellipsis added on the way cannot be what pushes it
/// over.
const MESSAGE_TEXT_MAX: usize = 4000;
const ACK_TEXT_MAX: usize = 190;

/// `text`, cut to `max` characters with an ellipsis if it had to be.
///
/// Characters and not bytes: the limit is on the API's side and is counted in
/// characters, and slicing bytes would split one in half and produce a body that
/// is not valid text at all.
fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    text.chars().take(max).collect::<String>() + "…"
}

/// The `sendMessage` body for one outgoing message.
fn send_body(out: &Outbound) -> Value {
    let mut body = json!({
        "chat_id": chat_id(&out.chat),
        "text": clip(&out.text, MESSAGE_TEXT_MAX),
    });
    if !out.buttons.is_empty() {
        body["reply_markup"] = json!({ "inline_keyboard": keyboard(&out.buttons) });
    }
    body
}

/// A chat id on the wire.
///
/// The model carries ids as text because the next channel will not number its
/// chats, but this API distinguishes a numeric id from a channel's `@name` — so
/// anything that reads as a number is sent as one, and anything else is sent as
/// it was written.
fn chat_id(chat: &str) -> Value {
    let chat = chat.trim();
    match chat.parse::<i64>() {
        Ok(id) => json!(id),
        Err(_) => json!(chat),
    }
}

/// Button rows as this API spells them.
fn keyboard(rows: &[Vec<Button>]) -> Value {
    let rows: Vec<Value> = rows
        .iter()
        .map(|row| {
            row.iter()
                .map(|b| json!({ "text": b.label, "callback_data": b.data }))
                .collect::<Vec<_>>()
                .into()
        })
        .collect();
    rows.into()
}

/// What went wrong with one call.
enum Trouble {
    /// Retrying would fail identically. The bridge ends on this.
    Fatal(String),
    /// The network, the far side's mood, a cancelled poll. Retry.
    Transient(String),
    /// Rate limited, with the wait the far side asked for.
    SlowDown(String, Duration),
}

impl Trouble {
    fn reason(&self) -> &str {
        match self {
            Self::Fatal(why) | Self::Transient(why) | Self::SlowDown(why, _) => why,
        }
    }
}

/// Whether a refusal from the API is one that retrying could ever fix.
///
/// Two are answers about the *credential*: the token was rejected, or there is
/// no bot by that token at all. Everything else — a busy server, a chat that
/// blocked the bot, a message that was too long — is either transient or about
/// one message, and neither is a reason to take the bridge down.
///
/// **409 is the third, and it is unfixable for a different reason.** It means
/// another process is polling this same bot, and a token has exactly one queue:
/// two pollers do not each get the messages, they split them, so half of what
/// somebody sends reaches an app that is not the one they are looking at. Left
/// as transient, the two back off and retry against each other for as long as
/// both are running, and which of them hears any given message is a coin toss.
/// The far side ends the *older* poll when a new one arrives, so treating it as
/// terminal means the instance that was already there stands down and the one
/// just started keeps the bot — which is the way round somebody who has just
/// launched an app expects it.
fn is_fatal(error_code: Option<i64>) -> bool {
    matches!(error_code, Some(401) | Some(404) | Some(409))
}

/// The far side's envelope. Every method answers in this shape.
#[derive(Deserialize)]
struct ApiReply<T> {
    ok: bool,
    result: Option<T>,
    description: Option<String>,
    error_code: Option<i64>,
    parameters: Option<ApiParams>,
}

#[derive(Deserialize)]
struct ApiParams {
    retry_after: Option<u64>,
}

impl<T> ApiReply<T> {
    /// Unwrap the envelope, classifying a refusal by what it says.
    fn into_result(self) -> Result<T, Trouble> {
        let why = self
            .description
            .unwrap_or_else(|| "no reason given".to_string());
        match (self.ok, self.result) {
            (true, Some(result)) => Ok(result),
            // Accepted, but the payload is not the shape this build expects. A
            // schema that moved is not something a retry fixes, but it is also
            // not a broken token -- treated as transient so a field added
            // upstream costs a log line rather than the whole bridge.
            (true, None) => Err(Trouble::Transient("unreadable reply".to_string())),
            _ if is_fatal(self.error_code) => Err(Trouble::Fatal(why)),
            _ => match self.parameters.and_then(|p| p.retry_after) {
                Some(secs) => Err(Trouble::SlowDown(why, Duration::from_secs(secs))),
                None => Err(Trouble::Transient(why)),
            },
        }
    }
}

/// One POST, and the envelope it answered with.
///
/// A transport failure is transient by definition here: it is a socket, not an
/// answer, so there is nothing in it that says the token is wrong.
///
/// **It takes the bot and the method rather than a URL**, so that the one place
/// that knows the credential is also the one place that turns a failure into
/// words. Handed a finished URL it could not redact what it was about to
/// report, and the caller would have to remember to — which is the kind of
/// thing one of three callers eventually does not.
async fn call<T: for<'de> Deserialize<'de>>(
    client: &reqwest::Client,
    bot: &Telegram,
    method: &str,
    body: &Value,
) -> Result<ApiReply<T>, Trouble> {
    let spoiled = |e: reqwest::Error| Trouble::Transient(bot.redact(e.to_string()));
    let response = client
        .post(bot.url(method))
        .json(body)
        .send()
        .await
        .map_err(spoiled)?;
    response.json::<ApiReply<T>>().await.map_err(spoiled)
}

// ── The wire shapes this build reads ────────────────────────────────────────

#[derive(Deserialize)]
struct Me {
    username: Option<String>,
    first_name: String,
}

#[derive(Deserialize)]
struct Update {
    update_id: i64,
    message: Option<TgMessage>,
    callback_query: Option<TgCallback>,
}

#[derive(Deserialize)]
struct TgMessage {
    chat: TgChat,
    text: Option<String>,
}

#[derive(Deserialize)]
struct TgChat {
    id: i64,
}

#[derive(Deserialize)]
struct TgCallback {
    id: String,
    data: Option<String>,
    message: Option<TgMessage>,
}

impl Update {
    /// This update as the model's own event, or nothing.
    ///
    /// Nothing is for what nobody is waiting on an answer to: a press carrying
    /// no data, an update this build never asked for. **A message is different**
    /// — somebody typed it and is watching for a reply — so one whose content
    /// this build cannot read comes back as
    /// [`RemoteEvent::Unreadable`] rather than vanishing. A photo dropped in
    /// silence is indistinguishable, from the far side, from a bridge that has
    /// crashed.
    fn into_event(self) -> Option<RemoteEvent> {
        if let Some(press) = self.callback_query {
            let chat = press.message?.chat.id.to_string();
            return Some(RemoteEvent::Pressed {
                chat,
                press_id: press.id,
                data: press.data?,
            });
        }
        let message = self.message?;
        let chat = message.chat.id.to_string();
        // A caption is not read as the message either. It is the text *beside* a
        // photo, so taking it alone would hand the agent "fix this" with nothing
        // to look at — a prompt about a picture that never travelled, which is a
        // worse answer than saying the picture did not travel.
        Some(match message.text {
            Some(text) => RemoteEvent::Message { chat, text },
            None => RemoteEvent::Unreadable { chat },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_update(text: &str) -> Option<RemoteEvent> {
        serde_json::from_str::<Update>(text).unwrap().into_event()
    }

    /// The failure this exists to prevent is silent and routine: an HTTP
    /// client names the URL it was talking to, and this API puts the token in
    /// the path, so one dropped connection prints a working credential.
    #[test]
    fn nothing_said_about_a_failure_carries_the_token() {
        let bot = Telegram::new("8729105146:AAHbQn".into()).with_root("https://example.test");
        let leaked = format!(
            "error sending request for url ({}): connection closed",
            bot.url("getUpdates")
        );
        let said = bot.redact(leaked);
        assert!(!said.contains("AAHbQn"), "got {said:?}");
        assert!(said.contains("<token>"), "got {said:?}");
        // What is left still says what went wrong and where.
        assert!(said.contains("getUpdates") && said.contains("connection closed"));
    }

    /// Substituting the empty string matches between every character and would
    /// turn one line into a wall of markers. Nothing gets this far, and the
    /// guard costs one comparison.
    #[test]
    fn a_blank_token_redacts_nothing() {
        let bot = Telegram::new(String::new());
        assert_eq!(bot.redact("plain words".into()), "plain words");
    }

    #[test]
    fn a_method_url_carries_the_token() {
        let bot = Telegram::new("123:abc".into()).with_root("https://example.test");
        assert_eq!(
            bot.url("getUpdates"),
            "https://example.test/bot123:abc/getUpdates"
        );
    }

    /// The two refusals that are answers about the token, and nothing else.
    /// Taking the bridge down on a busy server would mean a minute of the far
    /// side's trouble costs the user their bridge until they restart the app.
    #[test]
    fn only_what_retrying_cannot_fix_is_fatal() {
        // The credential.
        assert!(is_fatal(Some(401)));
        assert!(is_fatal(Some(404)));
        // Another process polling the same bot. A token has one queue, so two
        // pollers split the messages rather than each getting them -- retrying
        // is two instances taking turns losing half of what is sent.
        assert!(is_fatal(Some(409)));
        // Everything else is the far side's weather, or about one message.
        assert!(!is_fatal(Some(429)));
        assert!(!is_fatal(Some(500)));
        assert!(!is_fatal(Some(403)));
        assert!(!is_fatal(None));
    }

    #[test]
    fn a_rate_limit_carries_the_wait_it_asked_for() {
        let reply: ApiReply<Value> = serde_json::from_str(
            r#"{"ok":false,"error_code":429,"description":"Too Many Requests",
                "parameters":{"retry_after":7}}"#,
        )
        .unwrap();
        match reply.into_result() {
            Err(Trouble::SlowDown(_, wait)) => assert_eq!(wait, Duration::from_secs(7)),
            _ => panic!("a rate limit must be told apart from an ordinary failure"),
        }
    }

    #[test]
    fn a_rejected_token_ends_the_bridge_and_a_busy_server_does_not() {
        let rejected: ApiReply<Value> =
            serde_json::from_str(r#"{"ok":false,"error_code":401,"description":"Unauthorized"}"#)
                .unwrap();
        assert!(matches!(rejected.into_result(), Err(Trouble::Fatal(_))));

        let busy: ApiReply<Value> =
            serde_json::from_str(r#"{"ok":false,"error_code":500,"description":"Bad Gateway"}"#)
                .unwrap();
        assert!(matches!(busy.into_result(), Err(Trouble::Transient(_))));
    }

    #[test]
    fn a_text_message_becomes_a_message_event() {
        assert_eq!(
            parse_update(r#"{"update_id":9,"message":{"chat":{"id":-100123},"text":"hello"}}"#),
            Some(RemoteEvent::Message {
                chat: "-100123".to_string(),
                text: "hello".to_string(),
            })
        );
    }

    #[test]
    fn a_press_carries_the_chat_it_was_pressed_in() {
        assert_eq!(
            parse_update(
                r#"{"update_id":9,"callback_query":{"id":"cb1","data":"p:3:0",
                    "message":{"chat":{"id":77}}}}"#
            ),
            Some(RemoteEvent::Pressed {
                chat: "77".to_string(),
                press_id: "cb1".to_string(),
                data: "p:3:0".to_string(),
            })
        );
    }

    /// **A message somebody typed is never dropped in silence.** They are
    /// watching for a reply, and from the far side a photo that gets no answer
    /// is indistinguishable from a bridge that has crashed. A caption is not
    /// read as the message either: handing the agent "fix this" with no picture
    /// is a worse answer than saying the picture did not travel.
    #[test]
    fn a_message_this_build_cannot_read_still_comes_back() {
        for update in [
            r#"{"update_id":9,"message":{"chat":{"id":1},"sticker":{}}}"#,
            r#"{"update_id":9,"message":{"chat":{"id":1},"photo":[],"caption":"fix this"}}"#,
            r#"{"update_id":9,"message":{"chat":{"id":1},"voice":{}}}"#,
        ] {
            assert_eq!(
                parse_update(update),
                Some(RemoteEvent::Unreadable {
                    chat: "1".to_string()
                }),
                "for {update}"
            );
        }
    }

    /// What *is* dropped: updates nobody is waiting on an answer to.
    #[test]
    fn an_update_nobody_is_waiting_on_is_dropped() {
        assert_eq!(parse_update(r#"{"update_id":9}"#), None);
        // A press with no data has nothing to act on -- and nothing was typed,
        // so nobody is watching a chat for the reply.
        assert_eq!(
            parse_update(
                r#"{"update_id":9,"callback_query":{"id":"c","message":{"chat":{"id":1}}}}"#
            ),
            None
        );
    }

    /// The model carries ids as text so a channel that does not number its chats
    /// still fits, but this API tells a numeric id from an `@name`.
    #[test]
    fn a_numeric_chat_id_is_sent_as_a_number() {
        assert_eq!(chat_id(" -100123 "), json!(-100123_i64));
        assert_eq!(chat_id("@onehand_channel"), json!("@onehand_channel"));
    }

    #[test]
    fn a_message_with_no_buttons_carries_no_keyboard() {
        let body = send_body(&Outbound::text("5".into(), "hi"));
        assert_eq!(body["chat_id"], json!(5));
        assert_eq!(body["text"], json!("hi"));
        assert!(body.get("reply_markup").is_none());
    }

    /// Rows are the unit the far side lays out, and the model's rows have to
    /// survive as rows: a refusal folded onto the same line as the two grants is
    /// the button nobody meant to be next to.
    #[test]
    fn button_rows_stay_rows() {
        let out = Outbound {
            chat: "5".into(),
            text: "pick".into(),
            buttons: vec![
                vec![
                    Button {
                        label: "Allow".into(),
                        data: "p:1:0".into(),
                    },
                    Button {
                        label: "Always".into(),
                        data: "p:1:1".into(),
                    },
                ],
                vec![Button {
                    label: "Deny".into(),
                    data: "p:1:2".into(),
                }],
            ],
        };
        let keyboard = &send_body(&out)["reply_markup"]["inline_keyboard"];
        assert_eq!(keyboard.as_array().unwrap().len(), 2);
        assert_eq!(keyboard[0].as_array().unwrap().len(), 2);
        assert_eq!(keyboard[1][0]["callback_data"], json!("p:1:2"));
        assert_eq!(keyboard[0][1]["text"], json!("Always"));
    }
}
