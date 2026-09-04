//! The right-hand Workbench: the Editor and Files modes.
//!
//! One dock panel with mutually exclusive modes: the two are ways of reaching
//! the same files, so they share a dock rather than competing for width. Neovim
//! is a third mode and is deliberately absent: it needs the terminal parity
//! work this build stopped short of, and a half-working one would be worse than
//! an honest gap.

pub mod panel;

pub use panel::{EDITOR_MODE, FILES_MODE, NEOVIM_MODE, Workbench, WorkbenchMode};
