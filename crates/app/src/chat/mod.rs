//! The chat pane: the dock's centre panel and the sessions it shows.
//!
//! - [`session`] — [`ChatSession`], one entity per live conversation.
//! - [`conversation`] — [`Conversation`](conversation::Conversation), everything
//!   the pane knows about one session: its phase, and the state that belongs
//!   to it alone.
//! - [`pane`] — [`ChatPane`], the dock `Panel` that renders whichever session
//!   the rail has selected. A coordinator: it decides *which* conversation is
//!   showing and draws it, and owns almost nothing about any one of them.
//!
//! - [`viewport`] — how one transcript looks on screen: the run layout the
//!   virtual list draws, its scroll position, and the find bar.
//! - [`transcript`] — one element per `ChatItem`. Its block structure is the
//!   transcript's own design language; every colour, radius and size in it is
//!   read from `cx.theme()` at the call site, because the component library's
//!   theme is this app's look and a second palette would only drift from it.

pub mod composer;
pub mod conversation;
pub mod pane;
pub mod session;
pub mod transcript;
pub mod viewport;

pub use pane::ChatPane;
