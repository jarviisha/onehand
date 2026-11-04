//! A host is another machine, reached over SSH, that runs ACP sessions on
//! behalf of a laptop. The laptop and the host exchange newline-delimited JSON
//! lines, and each host session keeps an append-only journal of what happened
//! in it. `wire` is what crosses between the two; `journal` is what the host
//! keeps; `client` is the laptop's end. None of them needs a GUI.

pub mod client;
pub mod journal;
pub mod sshconf;
pub mod wire;
