//! The bridge that lets a device outside this machine reach the app.
//!
//! Split so that "add another channel" is a new file rather than a second set of
//! hooks running through the front end:
//!
//! - [`types`] — the neutral message model plus the [`types::RemoteChannel`]
//!   trait. Nothing here names a channel and nothing here names a front end.
//! - [`access`] — who is allowed to reach the app, and the rule that anyone else
//!   is answered with silence.
//! - [`command`] — the little language a chat drives the app with.
//! - [`press`] — what a button means, packed small enough to survive the trip.
//!
//! Wire implementations and their secret loading live in built-in plugins.
//!
//! Everything except the channel itself is pure and tested. The channel is the
//! ACP client's shape: a serve loop folded into the stream it returns, so a
//! front end drives it however it likes and dropping the stream ends it.

pub mod access;
pub mod command;
pub mod press;
pub mod types;

pub use access::{is_allowed, is_silently_ignored};
pub use command::{Aim, RemoteCommand};
pub use press::Press;
pub use types::{
    Button, ChatId, Outbound, RemoteChannel, RemoteEvent, RemoteRequest, ReqRx, ReqTx,
};
