//! A host is another machine, reached over SSH, that runs ACP sessions on
//! behalf of a laptop. The laptop and the host exchange newline-delimited JSON
//! lines, and each host session keeps an append-only journal of what happened
//! in it. `wire` is what crosses between the two; `journal` is what the host
//! keeps. Both ends link them, and neither needs a GUI.

pub mod journal;
pub mod wire;
