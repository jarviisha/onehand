//! What every crate of Onehand draws with: a button that answers the
//! pointer, the derivation of status ink, the surface a dock is drawn on, the
//! shared sizes, and the pieces a page repeats.
//!
//! Shared rather than the app's own because the feature crates draw too and
//! cannot reach into the binary: one wrapper here is what keeps a button in
//! the Editor and a button in Settings the same button.

// Nothing here is `pub` unless the binary names it: `dead_code` stops at a
// `pub` item in a library, so one that lost its last caller looks exactly like
// a working feature.
#![warn(unreachable_pub)]

mod badge;
mod button;
mod choice;
mod choice_mark;
mod confirm;
mod control_size;
mod field;
mod icon;
mod ink;
mod list_detail;
mod menu;
mod notice;
mod page;
mod prose;
mod resize;
mod row;
mod row_actions;
mod strip;
mod surface;
mod tabs;
pub mod toast;
mod toggle;
pub mod tokens;
mod tooltip;
pub use badge::{badge, badge_edge, label_badge, label_tint};
pub use button::{Named, Quiet, Refuses, action, fold_toggle};
pub use choice::{choice, choice_track, picker, switch};
pub use choice_mark::choice_mark;
pub use confirm::{Confirm, confirm_on};
pub use control_size::{ControlSize, Short, Tall, TallIcon};
pub use field::{field, search_input, text_area, text_input};
pub use icon::AppIcon;
pub use ink::{
    StatusInk, Tint, control_fill, control_fill_of, faint_rule, meta_ink, middle_ink, neutral_tint,
    status_hue, status_ink, tint, tint_over,
};
pub use list_detail::{ListDetail, ListWidths, back_link, list_detail};
pub use menu::{
    menu_below, menu_command, menu_item, menu_label, menu_row, menu_still, split_action,
};
pub use notice::{hint, status_line};
pub use onehand_core::worktree::removal::Process;
pub use page::{page_head, page_search, page_top};
pub use prose::{no_code_label, unedged_code};
pub use resize::{edge_handle, h_resizable, v_resizable};
pub use row::{mark_slot, row, tree_row};
pub use row_actions::{HoverSlot, hover_slot};
pub use strip::{mode_strip, strip, strip_control, workbench_strip};
pub use surface::{card_box, dock_surface, scrollbar, well};
pub use tabs::{
    Measured, TAB_MAX_W, TAB_MENU_CAP, TabStrip, TabWidths, measure_tab, measure_width, tab_chip,
    tab_close, tab_more, tab_select, tab_strip, tab_width, unmeasured,
};
pub use toggle::toggle;
pub use tooltip::{put_tooltips_away, tip, tooltip_guard};
