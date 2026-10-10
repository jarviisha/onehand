//! The right-hand Workbench: Editor, Markdown, Issues, Plugins and Neovim.
//!
//! One dock panel with mutually exclusive modes. Three are ways of reaching the
//! same files — browsing the project and editing what you pick, reading the
//! documents among them, and a real editor in a PTY, last because it is a
//! program of its own — and two are the project's issues and the plugin list,
//! so they share a dock rather than competing for width.

pub mod panel;

pub use panel::{EDITOR_MODE, ISSUES_MODE, MARKDOWN_MODE, NEOVIM_MODE, Workbench, WorkbenchEvent};
