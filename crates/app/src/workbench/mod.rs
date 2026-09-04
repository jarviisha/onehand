//! The right-hand Workbench: Editor, Files, Markdown and Neovim.
//!
//! One dock panel with mutually exclusive modes. They are four ways of reaching
//! the same files — editing one, browsing them all, reading the documents among
//! them, and a real editor in a PTY — so they share a dock rather than competing
//! for width.

pub mod panel;

pub use panel::{EDITOR_MODE, FILES_MODE, MARKDOWN_MODE, NEOVIM_MODE, Workbench, WorkbenchMode};
