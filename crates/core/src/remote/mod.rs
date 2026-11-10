//! The bridge that lets a device outside this machine reach the app.
//!
//! Split so that "add another channel" is a new file rather than a second set of
//! hooks running through the front end:
//!
//! - [`types`] — the neutral message model plus the [`types::RemoteChannel`]
//!   trait. Nothing here names a channel and nothing here names a front end.
//! - [`command`] — the little language a chat drives the app with.
//! - [`press`] — what a button means, packed small enough to survive the trip.
//! - [`chats`] — who is allowed to reach the app at all, where each chat types
//!   and what each has asked to hear about, and every reply that reports or
//!   changes either.
//! - [`drive`] — what happens next: a line or a press in, what it asks of a
//!   session, and what is said back.
//! - [`link`] — what a prompt, a stop, a press or a look at the pickers from
//!   outside does to one session's drive, and what its news says on a chat.
//!
//! Wire implementations and their secret loading live in feature crates.
//!
//! Everything except the channel itself is decided here rather than in a front
//! end, and tested. [`drive::RemoteDrive`] holds the one piece that carries
//! state, [`chats::Chats`], and changes it; a front end reads it and carries out
//! what the drive answers. The channel is the
//! ACP client's shape: a serve loop folded into the stream it returns, so a
//! front end drives it however it likes and dropping the stream ends it.

pub mod chats;
pub mod command;
pub mod drive;
pub mod link;
pub mod press;
pub mod types;

pub use chats::{
    archive_listing, scan_archives, Announcement, ArchiveRow, Chats, Origin, RemoteSession,
};
pub use command::{Aim, RemoteCommand, HELP};
pub use drive::{Effect, Handled, Input, RemoteDrive, Stopped};
pub use press::Press;
pub use types::{
    Button, ChatId, Outbound, RemoteChannel, RemoteEvent, RemoteRequest, ReqRx, ReqTx,
};
