//! The message model every remote channel speaks, and the trait one implements.
//!
//! Neutral on purpose: nothing here names Telegram, and nothing here names a
//! front end. A channel translates its own wire format into these four events
//! and reads these two requests back, which is what makes "add Discord" a new
//! file rather than a second set of hooks through the app.
//!
//! Serde-free for the same reason the ACP data model is: a channel's wire format
//! is its own business, and a model that carried one channel's field names would
//! be that channel's model wearing a neutral name.

use futures::stream::Stream;
use tokio::sync::mpsc;

/// A conversation on the far side of a channel, as that channel names it.
///
/// A string rather than a number: Telegram numbers its chats, the next channel
/// will use a snowflake or an account name, and a bridge that has to know which
/// is which is a bridge with exactly one channel in it. The app never does
/// arithmetic on one — it compares it against the allow list and hands it back.
pub type ChatId = String;

/// The channel the app uses to push work into a live remote channel. Unbounded
/// so `send` is synchronous and callable from a non-async update, exactly as the
/// ACP request channel is.
pub type ReqTx = mpsc::UnboundedSender<RemoteRequest>;
pub type ReqRx = mpsc::UnboundedReceiver<RemoteRequest>;

/// One button offered under an outgoing message.
///
/// `data` is what comes back on the press, and it is small on purpose: a channel
/// is free to cap it (Telegram allows 64 bytes), so nothing that has to survive
/// the round trip may be longer than an identifier and two numbers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Button {
    pub label: String,
    pub data: String,
}

/// Something the app wants said outside itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outbound {
    pub chat: ChatId,
    pub text: String,
    /// Buttons, one inner list per row. Empty is an ordinary message.
    ///
    /// Rows rather than a flat list because a row is the unit a channel lays
    /// out, and deciding how to wrap is a decision about the *answer* — one
    /// refusal on a line of its own, away from the two grants — not about the
    /// screen it lands on.
    pub buttons: Vec<Vec<Button>>,
}

impl Outbound {
    /// A message with nothing to press.
    pub fn text(chat: ChatId, text: impl Into<String>) -> Self {
        Self {
            chat,
            text: text.into(),
            buttons: Vec::new(),
        }
    }
}

/// What the app asks a running channel to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteRequest {
    Send(Outbound),
    /// Tell the channel a button press has been dealt with.
    ///
    /// Its own request rather than a flag on the reply, because the two are
    /// answers to different questions and arrive at different times: the
    /// acknowledgement stops the far side spinning, and the reply says what
    /// happened. A channel with nothing to acknowledge simply ignores it.
    Ack {
        press_id: String,
        text: Option<String>,
    },
}

/// What a running channel reports back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteEvent {
    /// The channel is up and the credentials were accepted. `name` is what the
    /// far side calls this bot, which is the one thing worth saying about a
    /// connection nobody can see.
    Connected { name: String },
    /// Someone wrote to the bot.
    Message { chat: ChatId, text: String },
    /// Someone pressed a button on a message the app sent.
    ///
    /// `data` is what [`Button::data`] carried; `press_id` is what an
    /// [`RemoteRequest::Ack`] has to echo.
    Pressed {
        chat: ChatId,
        press_id: String,
        data: String,
    },
    /// The channel is not coming back on its own.
    ///
    /// **Only for failures retrying cannot fix** — a rejected token, a malformed
    /// endpoint. A dropped connection, a timeout or a rate limit is the normal
    /// condition of a long poll and is retried inside the channel, because a
    /// bridge that gave up on the first bad minute of wifi would need a person
    /// at the keyboard to come back, which is the one thing it exists to avoid
    /// needing.
    Disconnected(String),
}

/// One way of reaching a device outside the app.
///
/// The contract is the ACP client's, deliberately: the serve loop is folded into
/// the stream rather than spawned beside it, so the channel advances only while
/// something polls it and dropping the stream shuts the whole thing down. There
/// is no separate stop to remember, and no way to leave a poll running against a
/// bridge nobody is listening to.
pub trait RemoteChannel: Send + 'static {
    /// What this channel is called, for anything that has to name it.
    fn name(&self) -> &'static str;

    /// Run the channel, taking requests from `requests` and emitting events.
    fn connect(self, requests: ReqRx) -> impl Stream<Item = RemoteEvent> + Send;
}
