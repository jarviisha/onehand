//! The runtimes onehand's I/O runs on, apart from the window that draws it.
//!
//! What a session's drive asks of the world (an agent's process, its stream
//! of events) runs here, on runtimes this crate owns, and crosses to the
//! front end over channels that belong to no executor. Nothing here links a
//! GUI framework: another front end takes this crate as it is.
//!
//! - [`acp`]: the agent runtime every session's adapter runs on.

#![warn(unreachable_pub)]

pub mod acp;
