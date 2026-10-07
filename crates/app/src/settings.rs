//! Settings: appearance, this workspace, the agent menu, the connections,
//! the keymap.
//!
//! A modal, mounted by the shell so its rail row and its key share one dialog,
//! and sized to the window up to 960 × 680. The shell restores the previous focus when
//! it closes.

use crate::shell::Shell;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, AppContext, ClickEvent, Context, Div, Entity, InteractiveElement, IntoElement,
    ParentElement, SharedString, StatefulInteractiveElement as _, Styled, Window, div, rems,
};
use gpui_component::button::ButtonVariants;
use gpui_component::input::InputState;
use gpui_component::tag::Tag;
use gpui_component::{ActiveTheme, Icon, IconName, Sizable as _, StyledExt};
use onehand_core::config::AgentSpec;

mod agents;
mod pages;
use agents::agents_page;
pub use agents::{AgentCheck, check_key, draft_after_promote};
use pages::{appearance_page, connections_page, workspace_page};

/// The add/edit form's fields. `editing` is `Some(i)` when an existing agent is
/// being changed and `None` when a new one is being added, so one form serves
/// both without a second "mode" flag to keep in step with it.
pub struct AgentDraft {
    pub editing: Option<usize>,
    pub name: Entity<InputState>,
    pub command: Entity<InputState>,
    pub args: Entity<InputState>,
}

impl AgentDraft {
    pub fn new(window: &mut Window, cx: &mut App) -> Self {
        Self {
            editing: None,
            name: cx.new(|cx| InputState::new(window, cx).placeholder("Claude Code")),
            command: cx.new(|cx| InputState::new(window, cx).placeholder("npx")),
            args: cx.new(|cx| {
                InputState::new(window, cx).placeholder("-y @agentclientprotocol/claude-agent-acp")
            }),
        }
    }

    /// Load an existing agent into the form for editing.
    pub fn load(&mut self, idx: usize, spec: &AgentSpec, window: &mut Window, cx: &mut App) {
        self.editing = Some(idx);
        self.name
            .update(cx, |s, cx| s.set_value(&spec.name, window, cx));
        self.command
            .update(cx, |s, cx| s.set_value(&spec.command, window, cx));
        self.args
            .update(cx, |s, cx| s.set_value(spec.args_line(), window, cx));
    }

    /// Clear the form back to "adding a new agent".
    pub fn clear(&mut self, window: &mut Window, cx: &mut App) {
        self.editing = None;
        for field in [&self.name, &self.command, &self.args] {
            field.update(cx, |s, cx| s.set_value("", window, cx));
        }
    }

    /// Whether the form holds something not yet saved: a change to the agent it
    /// was opened on, or anything typed into a form adding a new one.
    pub fn dirty(&self, agents: &[AgentSpec], cx: &App) -> bool {
        match self.editing {
            Some(idx) => self.to_spec(cx).as_ref() != agents.get(idx),
            None => [&self.name, &self.command, &self.args]
                .iter()
                .any(|field| !field.read(cx).value().trim().is_empty()),
        }
    }

    /// The spec this form describes, or `None` while it is not saveable.
    ///
    /// Name and command must both be non-blank -- that is the rule Save is
    /// greyed out by. Args split on whitespace.
    pub fn to_spec(&self, cx: &App) -> Option<AgentSpec> {
        let name = self.name.read(cx).value().trim().to_string();
        let command = self.command.read(cx).value().trim().to_string();
        if name.is_empty() || command.is_empty() {
            return None;
        }
        Some(AgentSpec {
            name,
            command,
            // Core's parser, paired with `args_line` on the way in, so an
            // argument containing a space survives being edited.
            args: onehand_core::config::split_args(&self.args.read(cx).value()),
        })
    }
}

/// What deleting an agent does to a form left open on one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DraftShift {
    /// The form is about an agent the deletion did not move.
    Keep,
    /// The agent being edited is the one that went.
    Clear,
    /// The form's agent slid down into the hole above it.
    MoveTo(usize),
}

/// Where a form open on `editing` belongs once the agent at `removed` is gone.
///
/// `AgentDraft::editing` is a *position* in the agent list, and deleting shifts
/// every position after the one removed. Left alone, a form opened on the third
/// agent while the first is deleted goes on pointing at index 2 -- a different
/// agent now, which Save then overwrites. Deleting the agent being edited is
/// worse: the index falls off the end and Save appends, putting back the agent
/// the user had just asked to be rid of.
///
/// A position and not an id because an agent is identified by a name the user
/// writes and renaming one is the whole point of the form. So the rule is to
/// correct the position at the one place the list shifts, and it is pure so
/// that place can be checked without a window.
pub fn draft_shift(editing: Option<usize>, removed: usize) -> DraftShift {
    match editing {
        Some(editing) if editing == removed => DraftShift::Clear,
        Some(editing) if editing > removed => DraftShift::MoveTo(editing - 1),
        Some(_) | None => DraftShift::Keep,
    }
}

/// A setting's line about itself, from a fixed sentence.
pub(crate) fn about(text: &'static str) -> Option<AnyElement> {
    Some(text.into_any_element())
}

/// A page's head: its name, the same word its nav row carries, what the page
/// governs, and one line saying what the page is about.
///
/// **The scope is said on every page, in the same place.** What is in here
/// belongs to different reaches -- the app as a whole, or the one workspace
/// this window holds -- and a setting that reads the same either way is one
/// somebody changes believing it is the other.
pub(crate) fn page_head(
    name: &'static str,
    about: &'static str,
    scope: impl Into<SharedString>,
    cx: &App,
) -> impl IntoElement {
    div()
        .v_flex()
        .gap_1()
        .child(
            div()
                .h_flex()
                .items_center()
                .gap_2()
                .child(page_title(name))
                .child(Tag::secondary().small().child(scope.into())),
        )
        .child(
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(about),
        )
}

/// The scope tag for a page whose settings reach every window and workspace.
pub(crate) const APP: &str = "App";

/// A group of settings on a page.
///
/// **No box around it.** The page is one surface and a group is a run of
/// settings on it; what separates two groups is a hairline and a heading, the
/// way a document separates sections. A group with no title is the page's
/// first, sitting directly under the page's own head, and takes no rule --
/// a line between a heading and the first thing it heads divides what belongs
/// together.
pub(crate) fn section(title: Option<&'static str>, about: Option<SharedString>, cx: &App) -> Div {
    let muted = cx.theme().muted_foreground;
    div()
        .v_flex()
        .gap_5()
        .w_full()
        .when_some(title, |group, title| {
            group
                .pt_6()
                .border_t_1()
                .border_color(cx.theme().border)
                .child(
                    div()
                        .v_flex()
                        .gap_1()
                        .child(div().text_base().font_medium().child(title))
                        .when_some(about, |head, about| {
                            head.child(div().text_sm().text_color(muted).child(about))
                        }),
                )
        })
}

/// One setting, stacked: its name, a line about it, and the control under
/// both at the column's full width -- so a field is as wide as what can be
/// typed into it, and a long description wraps above it rather than beside it.
pub(crate) fn field(
    title: impl Into<SharedString>,
    about: Option<AnyElement>,
    control: impl IntoElement,
    cx: &App,
) -> Div {
    div()
        .v_flex()
        .gap_2()
        .w_full()
        .child(
            div()
                .v_flex()
                .gap_0p5()
                .child(div().text_sm().child(title.into()))
                .children(about.map(|about| {
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(about)
                })),
        )
        .child(control)
}

/// One entry in a list: what it is on the left, what can be done to it on the
/// right. For things of which there are many -- agents, commands -- where the
/// stacked [`field`] would make every entry three lines tall.
pub(crate) fn list_row(
    title: impl Into<SharedString>,
    about: Option<AnyElement>,
    control: impl IntoElement,
    cx: &App,
) -> Div {
    div()
        .h_flex()
        .items_center()
        .justify_between()
        .gap_4()
        .w_full()
        .child(
            div()
                .v_flex()
                .gap_0p5()
                .flex_1()
                .min_w_0()
                .child(div().text_sm().child(title.into()))
                .children(about.map(|about| {
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(about)
                })),
        )
        .child(
            div()
                .flex_none()
                .h_flex()
                .items_center()
                .gap_2()
                .child(control),
        )
}

/// Which page Settings is showing.
///
/// A page per group of control, and two of them used to be their own
/// surface: the agent list and the keyboard table were separate
/// dialogs behind separate rail rows, so "where is that setting" had three
/// answers and which one was right depended on which row somebody remembered.
/// One page, one way in, and the rail's footer is one row instead of three.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum SettingsPage {
    /// Appearance first, and it is the default, because it is the only page
    /// here that changes what the app *looks* like and so the one somebody
    /// opening Settings without a specific errand came for.
    #[default]
    Appearance,
    Workspace,
    Agents,
    /// The services outside the checkout. A page of its own rather than a
    /// line under unattended runs, because more of them are coming and each
    /// is used by more than the one feature that happened to need it first.
    /// Named for what it holds -- connectors and who each is signed in as --
    /// and not for a protocol none of them speaks yet.
    Connections,
    Shortcuts,
}

impl SettingsPage {
    pub const ALL: [Self; 5] = [
        Self::Appearance,
        Self::Workspace,
        Self::Agents,
        Self::Connections,
        Self::Shortcuts,
    ];

    /// The name in the nav and at the head of the page, which are one string on
    /// purpose: a nav that says one word and a heading that says another leaves
    /// the reader working out whether they landed where they clicked.
    fn label(self) -> &'static str {
        match self {
            Self::Appearance => "Appearance",
            Self::Workspace => "Workspace",
            Self::Agents => "Agents",
            Self::Connections => "Connections",
            Self::Shortcuts => "Shortcuts",
        }
    }

    /// The mark leading its nav row.
    fn icon(self) -> Icon {
        match self {
            Self::Appearance => Icon::new(IconName::Palette),
            Self::Workspace => Icon::new(IconName::FolderClosed),
            Self::Agents => Icon::new(IconName::Bot),
            Self::Connections => Icon::new(IconName::Network),
            Self::Shortcuts => Icon::new(crate::icons::Icon::Keyboard),
        }
    }
}

/// One row in the nav column.
///
/// A div and not the app's button wrapper, for the reason the rail's rows are:
/// a full-width [`gpui_component::button::Button`] centres its own content and
/// that is not style-refinable from outside, so a column of them reads as a row
/// of banners rather than as a list. Each leads with its page's mark on the
/// rail's own icon column, so the column reads as the same kind of list the
/// rail beside it is.
fn nav_row(page: SettingsPage, current: SettingsPage, handle: &Entity<Shell>, cx: &App) -> Div {
    let (accent, accent_fg, muted, radius) = (
        cx.theme().accent,
        cx.theme().accent_foreground,
        cx.theme().muted_foreground,
        cx.theme().radius,
    );
    let handle = handle.clone();
    let selected = page == current;

    div().w_full().child(
        div()
            .id(page.label())
            .h_flex()
            .items_center()
            .gap_2()
            .w_full()
            .px_2()
            .py_1p5()
            .rounded(radius)
            .text_sm()
            .cursor_pointer()
            .map(|row| match selected {
                true => row.bg(accent).text_color(accent_fg),
                false => row
                    .text_color(muted)
                    .hover(move |row| row.bg(accent.opacity(0.5))),
            })
            .child(page.icon().small())
            .child(page.label())
            .on_click(move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                handle.update(cx, |shell, cx| shell.show_settings_page(page, cx));
            }),
    )
}

/// The heading a page opens with -- the same word its nav row carries.
pub(crate) fn page_title(name: &'static str) -> impl IntoElement {
    div().text_xl().font_medium().child(name)
}

/// Settings: appearance, this workspace, the agent menu, the connections, the
/// keymap.
///
/// **A modal, and a roomy one.** At the few hundred pixels a dialog usually
/// takes, the keymap and the agent form scrolled inside a box; drawn in the
/// panels' place instead, it had to be left by navigating, and every command
/// had to be told it was not on screen. A modal up to 960 × 680 keeps enough
/// room and the one way out, without a near-full-window box that left a
/// one-control page looking lost in it.
///
/// **A nav column and a page, not one scroll.** What is in here belongs to
/// three different scopes -- the theme is app-wide, the name and the storage
/// binding are this workspace's, the agent list is every workspace's -- and
/// stacked in one column the only thing saying so was a row of `text_xs`
/// labels. A page per scope is the shape that says it without a sentence.
///
/// **One surface, and no rule where space already divides.** The nav and the
/// page are not separated by a border, nor the page by a header bar: the gap
/// between them is the edge. The only lines left are the dialog's own edge and
/// the ones between groups on a page.
///
/// **There is no footer, and no control here sits away from what it acts on.**
/// The pair that binds and unbinds storage was in one, three items below the
/// folder it names -- and a `.primary()` button at the foot of a settings
/// page reads as *Save*, while that one opens a folder picker and re-points
/// where the workspace is written.
///
/// **Drawn by a view of its own** ([`SettingsView`]) rather than built inside
/// the shell's render: every page reads the shell, and the shell is the entity
/// being updated for as long as its own render runs, so reading it from there
/// is a panic. A child view renders after the shell's render has returned --
/// and the dialog's content builder runs inside that render, so the view is
/// what keeps the dialog able to read the shell at all.
pub struct SettingsView {
    shell: gpui::WeakEntity<Shell>,
}

impl SettingsView {
    pub fn new(shell: gpui::WeakEntity<Shell>) -> Self {
        Self { shell }
    }
}

impl gpui::Render for SettingsView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        match self.shell.upgrade() {
            Some(handle) => settings(&handle, cx),
            None => div().into_any_element(),
        }
    }
}

/// The modal around [`SettingsView`], sized to the window it opens in.
///
/// **960 × 680 at most**: room for the nav and a page holding a stacked field
/// at a comfortable width, without the near-full-window box that left a
/// one-control page looking lost in it.
///
/// **Clamped to the frame on both axes**, because the library centres a
/// dialog by subtracting half its width from half the viewport's and never
/// clamps: a box wider than the window starts at a negative x, with the nav off
/// the left edge and the ✕ off the right and nothing to scroll either back. The
/// height is fixed rather than grown to the page, so the box does not jump size
/// between pages, and the top margin centres it vertically instead of the
/// library's tenth-of-the-viewport drop, which would push a box this tall off
/// the bottom.
///
/// The library's own padding is taken off (`p_0`) because the view pads its
/// own nav and page, and its ✕ is off too, since that one is a plain library
/// button that draws the arrow cursor; the view carries its own in the corner.
pub fn dialog(
    view: Entity<SettingsView>,
    window: &Window,
    cx: &mut Context<Shell>,
) -> gpui_component::dialog::Dialog {
    let frame = window.viewport_size();
    let width = (frame.width - gpui::px(64.)).clamp(gpui::px(360.), gpui::px(960.));
    let height = (frame.height - gpui::px(64.)).clamp(gpui::px(240.), gpui::px(680.));
    let top = ((frame.height - height) / 2.).max(gpui::px(0.));
    let handle = cx.entity();

    gpui_component::dialog::Dialog::new(cx)
        .w(width)
        .margin_top(top)
        .p_0()
        .close_button(false)
        .on_close(move |_, window, cx| {
            handle.update(cx, |shell, cx| shell.request_close_settings(window, cx))
        })
        .content(move |content, _: &mut Window, _: &mut App| {
            content.child(div().h(height).w_full().child(view.clone()))
        })
}

fn settings(handle: &Entity<Shell>, cx: &App) -> AnyElement {
    let handle = handle.clone();
    let current = handle.read(cx).settings_page();
    let focus = handle.read(cx).settings_focus();
    let muted = cx.theme().muted_foreground;
    let status = crate::theme::status_ink(cx);
    let note = handle.read(cx).settings_note(current, cx);
    let nav = SettingsPage::ALL
        .into_iter()
        .map(|page| nav_row(page, current, &handle, cx).into_any_element())
        .collect::<Vec<_>>();
    let page = match current {
        SettingsPage::Appearance => appearance_page(&handle, cx),
        SettingsPage::Workspace => workspace_page(&handle, cx),
        SettingsPage::Agents => agents_page(&handle, cx),
        SettingsPage::Connections => connections_page(cx),
        SettingsPage::Shortcuts => handle.read(cx).keymap_editor().into_any_element(),
    };

    div()
        // The caret has to be inside the page for Escape to reach it, and the
        // handle is the shell's because this is rebuilt every frame -- one made
        // here would be a new handle each time.
        .track_focus(&focus)
        // Escape pressed in one of the page's fields: the field claims the key
        // itself and lets the action travel outward when it has no use for it,
        // so a binding of ours on the key would never be reached from there.
        .on_action({
            let handle = handle.clone();
            move |_: &gpui_component::input::Escape, window, cx| {
                handle.update(cx, |shell, cx| shell.request_close_settings(window, cx));
            }
        })
        .relative()
        .size_full()
        .h_flex()
        .items_start()
        .child(
            div()
                .v_flex()
                .gap_0p5()
                .flex_none()
                .w(rems(13.5))
                .h_full()
                .px_4()
                .py_5()
                .child(
                    div()
                        .px_2()
                        .pb_2()
                        .text_sm()
                        .font_medium()
                        .child("Settings"),
                )
                .children(nav)
                // The build, at the foot of the nav: it is about the app
                // rather than about any one page.
                .child(div().flex_1())
                .child(
                    div()
                        .px_2()
                        .text_xs()
                        .text_color(muted)
                        .child(format!("onehand {}", env!("CARGO_PKG_VERSION"))),
                ),
        )
        .child(
            // The page scrolls, not the frame: the nav has to stay reachable
            // from the bottom of a long page, and the keymap is longer than
            // any window.
            div()
                .id("settings-page")
                .flex_1()
                .min_w_0()
                .h_full()
                .overflow_y_scroll()
                .child(
                    // A reading measure, so a wide window does not stretch a
                    // field across it, centred in whatever room is left.
                    div().h_flex().justify_center().w_full().child(
                        div()
                            .v_flex()
                            .w_full()
                            .max_w(rems(48.))
                            .px_8()
                            .py_6()
                            .child(page),
                    ),
                ),
        )
        // The way out, in the corner rather than on a title bar of its own: a
        // header row holding one button is a border and a strip of surface
        // spent on it.
        .child(
            div()
                .absolute()
                .top_3()
                .right_3()
                .h_flex()
                .items_center()
                .gap_2()
                // What the last change on this page did, beside the way out:
                // settings apply as they are made, so the only thing left to
                // say is whether the write took.
                .children(note.map(|note| {
                    let (line, ink) = match note {
                        Ok(()) => ("Saved".to_string(), status.success),
                        Err(why) => (format!("Not saved — {why}"), status.danger),
                    };
                    div()
                        .max_w(rems(24.))
                        .truncate()
                        .text_sm()
                        .text_color(ink)
                        .child(line)
                }))
                .child(
                    crate::controls::action("settings-close")
                        .small()
                        .ghost()
                        .icon(Icon::new(IconName::Close))
                        .tooltip("Close Settings")
                        .on_click({
                            let handle = handle.clone();
                            move |_: &ClickEvent, window, cx| {
                                handle.update(cx, |shell, cx| {
                                    shell.request_close_settings(window, cx)
                                });
                            }
                        }),
                ),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests;
