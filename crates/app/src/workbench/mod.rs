//! The right-hand Workbench: Editor, Markdown and Neovim.
//!
//! One dock panel with mutually exclusive modes. They are three ways of reaching
//! the same files — browsing the project and editing what you pick, reading the
//! documents among them, and a real editor in a PTY — so they share a dock
//! rather than competing for width.

pub mod panel;

pub use panel::{EDITOR_MODE, MARKDOWN_MODE, NEOVIM_MODE, Workbench, WorkbenchEvent};
