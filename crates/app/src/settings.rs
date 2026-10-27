//! Settings: appearance, this workspace, the agent menu, the connections,
//! the keymap.
//!
//! A page in the agent pane, opened by its rail row and its key, and left the
//! way every page is left.

use crate::shell::Shell;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, AppContext, ClickEvent, Context, Div, Entity, InteractiveElement, IntoElement,
    ParentElement, SharedString, StatefulInteractiveElement as _, Styled, Window, div,
};
use gpui_component::input::InputState;
use gpui_component::{ActiveTheme, Icon, IconName, Sizable as _, StyledExt};
use onehand_core::config::{AgentSpec, Preset};

mod agents;
mod pages;
use agents::agents_page;
pub(crate) use agents::kind_about;
pub use agents::{AgentCheck, check_key, draft_after_promote};
use pages::{appearance_page, connections_page, workspace_page};

/// The add/edit form's fields. `editing` is `Some(i)` when an existing agent is
/// being changed and `None` when a new one is being added, so one form serves
/// both without a second "mode" flag to keep in step with it.
pub struct AgentDraft {
    pub editing: Option<usize>,
    /// The built-in agent this is, or `None` for one written out in full.
    pub kind: Option<Preset>,
    pub name: Entity<InputState>,
    pub command: Entity<InputState>,
    pub args: Entity<InputState>,
    pub env: Entity<InputState>,
    /// The adapter version of the entry being edited, which the form has no
    /// field for and so must not drop.
    version: Option<String>,
}

impl AgentDraft {
    pub fn new(window: &mut Window, cx: &mut App) -> Self {
        let mut draft = Self {
            editing: None,
            kind: None,
            name: cx.new(|cx| InputState::new(window, cx)),
            command: cx.new(|cx| InputState::new(window, cx).placeholder("gemini")),
            args: cx.new(|cx| InputState::new(window, cx).placeholder("--experimental-acp")),
            env: cx.new(|cx| InputState::new(window, cx)),
            version: None,
        };
        draft.set_kind(Some(Preset::ClaudeCode), window, cx);
        draft
    }

    /// Make the form describe `kind`, its placeholders included: an example
    /// written for one agent reads as wrong under another's name.
    pub fn set_kind(&mut self, kind: Option<Preset>, window: &mut Window, cx: &mut App) {
        self.kind = kind;
        let (name, env) = match kind.map(Preset::info) {
            Some(info) => (
                info.shown_name.to_string(),
                format!("{}=~/.{}-work", info.account_var, info.binary),
            ),
            None => ("Gemini".to_string(), "NAME=value".to_string()),
        };
        for (field, hint) in [(&self.name, name), (&self.env, env)] {
            field.update(cx, |s, cx| s.set_placeholder(hint, window, cx));
        }
    }

    /// Load an existing agent into the form for editing.
    pub fn load(&mut self, idx: usize, spec: &AgentSpec, window: &mut Window, cx: &mut App) {
        self.editing = Some(idx);
        self.set_kind(spec.preset(), window, cx);
        self.version = spec.version.clone();
        let env = spec
            .env
            .iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect::<Vec<_>>();
        for (field, value) in [
            (&self.name, spec.name.clone()),
            (&self.command, spec.command.clone()),
            (&self.args, spec.args_line()),
            (&self.env, onehand_core::config::join_args(&env)),
        ] {
            field.update(cx, |s, cx| s.set_value(value, window, cx));
        }
    }

    /// Clear the form back to "adding a new agent".
    pub fn clear(&mut self, window: &mut Window, cx: &mut App) {
        self.editing = None;
        self.set_kind(Some(Preset::ClaudeCode), window, cx);
        self.version = None;
        for field in [&self.name, &self.command, &self.args, &self.env] {
            field.update(cx, |s, cx| s.set_value("", window, cx));
        }
    }

    /// The spec this form describes, or why it cannot be saved yet: the
    /// sentence Save refuses with.
    pub fn to_spec(&self, cx: &App) -> Result<AgentSpec, &'static str> {
        let value = |field: &Entity<InputState>| field.read(cx).value().to_string();
        // Every other entry's name: the one being edited may keep its own.
        let taken = crate::state::Shared::global(cx)
            .agents
            .iter()
            .enumerate()
            .filter(|(at, _)| Some(*at) != self.editing)
            .map(|(_, spec)| spec.name.as_str())
            .collect::<Vec<_>>();
        draft_spec(
            self.kind,
            &value(&self.name),
            &value(&self.command),
            &value(&self.args),
            &value(&self.env),
            self.version.clone(),
            &taken,
        )
    }
}

/// The agent a form's fields describe, or the sentence Save refuses with.
///
/// A built-in agent needs a name; one written out in full needs its command
/// too. The name must be none of `taken`, since sessions, runs and archives
/// find an agent by its name. The environment is one line of `NAME=value` words through core's
/// argument parser, paired with `join_args` on the way in, so a value holding
/// a space survives being edited; each word splits at its first `=`.
fn draft_spec(
    kind: Option<Preset>,
    name: &str,
    command: &str,
    args: &str,
    env: &str,
    version: Option<String>,
    taken: &[&str],
) -> Result<AgentSpec, &'static str> {
    let (name, command) = (name.trim().to_string(), command.trim().to_string());
    if taken.contains(&name.as_str()) {
        return Err("Another agent has that name");
    }
    if name.is_empty() && kind.is_some() {
        return Err("Name it first");
    }
    if name.is_empty() || (command.is_empty() && kind.is_none()) {
        return Err("Name it and give its command first");
    }
    let env = onehand_core::config::split_args(env)
        .into_iter()
        .map(|word| {
            word.split_once('=')
                .filter(|(key, _)| !key.is_empty())
                .map(|(key, value)| (key.to_string(), value.to_string()))
                .ok_or("Write each variable as NAME=value")
        })
        .collect::<Result<_, _>>()?;
    Ok(match kind {
        Some(preset) => AgentSpec {
            name,
            preset: Some(preset.into()),
            version,
            env,
            ..Default::default()
        },
        None => AgentSpec {
            name,
            command,
            args: onehand_core::config::split_args(args),
            env,
            ..Default::default()
        },
    })
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

/// A page's head: what the page governs, as a badge, and one line saying what
/// it is about. No title: the nav row beside it names the page, and the
/// head over the nav names Settings.
///
/// **The scope is said on every page, in the same place.** What is in here
/// belongs to different reaches -- the app as a whole, or the one workspace
/// this window holds -- and a setting that reads the same either way is one
/// somebody changes believing it is the other.
pub(crate) fn page_head(
    about: &'static str,
    scope: impl Into<SharedString>,
    cx: &App,
) -> impl IntoElement {
    div()
        .h_flex()
        .items_center()
        .gap_2()
        .child(
            onehand_ui::badge(
                scope,
                onehand_ui::Tint {
                    fill: cx.theme().secondary,
                    ink: cx.theme().secondary_foreground,
                },
            )
            .flex_none(),
        )
        .child(
            div()
                .min_w_0()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .text_color(onehand_ui::meta_ink(cx))
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
        .gap_4()
        .w_full()
        .when_some(title, |group, title| {
            group
                .pt_6()
                .border_t_1()
                .border_color(onehand_ui::faint_rule(cx))
                .child(
                    div()
                        .v_flex()
                        .gap_1()
                        .child(
                            div()
                                .text_size(crate::tokens::CHROME_TEXT)
                                .font_semibold()
                                .child(title),
                        )
                        .when_some(about, |head, about| {
                            head.child(div().text_xs().text_color(muted).child(about))
                        }),
                )
        })
}

/// One setting, stacked: the UI crate's, where a feature crate's form reaches it
/// too.
pub(crate) use onehand_ui::field;

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
        .gap_2()
        .w_full()
        .child(
            div()
                .v_flex()
                .gap_0p5()
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .text_size(crate::tokens::CHROME_TEXT)
                        .child(title.into()),
                )
                .children(about.map(|about| {
                    div()
                        .text_xs()
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

/// A run of [`list_row`]s, a hairline between each two, as a page's box parts
/// its rows: the entries of one list read as one list, not as loose lines.
pub(crate) fn list_rows(rows: impl IntoIterator<Item = AnyElement>, cx: &App) -> Div {
    crate::chat::pane::page_parts::ruled(
        rows.into_iter()
            .map(|row| div().py_2().child(row).into_any_element()),
        cx,
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
    let handle = handle.clone();
    let row = div()
        .id(page.label())
        .h_flex()
        .items_center()
        .gap_2()
        .w_full()
        .h(crate::tokens::ROW_H)
        .px_2()
        .rounded(cx.theme().radius)
        .cursor_pointer()
        .text_size(crate::tokens::CHROME_TEXT);

    // A list row as the rail's page rows are, in their fills and inks: the
    // chosen fill in its own ink, the rest muted at rest, the mark in one slot
    // down the column.
    div().w_full().child(
        crate::rail::page_fill(row, page == current, cx)
            .child(onehand_ui::mark_slot(page.icon().small()))
            .child(page.label())
            .on_click(move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                handle.update(cx, |shell, cx| shell.show_settings_page(page, cx));
            }),
    )
}

/// The width of Settings' nav column: its longest name beside its mark.
const NAV_W: gpui::Rems = gpui::rems(13.5);

/// Settings: appearance, this workspace, the agent menu, the connections, the
/// keymap.
///
/// **A page, not a modal.** It is somewhere to be: opened by its rail row and
/// its key, left by a session, another page, a key or a dock, as every page is,
/// and found again as it was. Esc does nothing on it; inside a field, Esc is
/// the field's.
///
/// **A nav column and a page, not one scroll.** What is in here belongs to
/// three different scopes -- the theme is app-wide, the name and the storage
/// binding are this workspace's, the agent list is every workspace's -- and
/// stacked in one column the only thing saying so was a row of `text_xs`
/// labels. A page per scope is the shape that says it without a sentence.
///
/// **There is no footer, and no control here sits away from what it acts on.**
/// Settings apply as they are made, and a write that fails says so in a toast.
///
/// **Drawn by a view of its own** ([`SettingsView`]) rather than built inside
/// the shell's render: every page reads the shell, and the shell is the entity
/// being updated for as long as its own render runs, so reading it from there
/// is a panic. A child view renders after the shell's render has returned.
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

fn settings(handle: &Entity<Shell>, cx: &App) -> AnyElement {
    let handle = handle.clone();
    let current = handle.read(cx).settings_page();
    let focus = handle.read(cx).settings_focus();
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
        // A command acting on a region this page hides (the Workbench editor's
        // save) excludes it, so a key typed here never reaches what is not on
        // screen.
        .key_context("Settings")
        .size_full()
        .v_flex()
        // Named in its own head, as every other page is, the pane's header
        // above it left bare. The head runs the pane's width, so its name
        // starts on the nav's edge.
        .child(crate::chat::pane::page_parts::page_top(
            None,
            [
                crate::chat::pane::page_parts::page_head("Settings", None, Vec::new(), cx)
                    .into_any_element(),
            ],
            cx,
        ))
        .child(
            div()
                .flex_1()
                .min_h_0()
                .h_flex()
                .items_start()
                .child(settings_nav(nav, &focus, cx))
                .child(settings_body(page)),
        )
        .into_any_element()
}

/// The nav column: a row per page, and the build at its foot.
fn settings_nav(nav: Vec<AnyElement>, focus: &gpui::FocusHandle, cx: &App) -> impl IntoElement {
    let muted = cx.theme().muted_foreground;
    div()
        .id("settings-nav")
        // The keyboard lands here when Settings opens. The handle is
        // the shell's because this is rebuilt every frame -- one made
        // here would be a new handle each time.
        .track_focus(focus)
        .v_flex()
        .gap_0p5()
        .flex_none()
        .w(NAV_W)
        .h_full()
        .px_8()
        .py_6()
        .pr_0()
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
        )
}

/// The page beside the nav.
fn settings_body(page: AnyElement) -> impl IntoElement {
    // The page scrolls, not the frame: the nav has to stay reachable
    // from the bottom of a long page, and the keymap is longer than
    // any window.
    div()
        .id("settings-page")
        .flex_1()
        .min_w_0()
        .h_full()
        .overflow_y_scroll()
        .px_8()
        .py_6()
        .child(
            // A reading measure, so a wide window does not stretch a
            // field across it, centred in whatever room is left.
            div().h_flex().justify_center().w_full().child(
                div()
                    .v_flex()
                    .w_full()
                    .max_w(crate::tokens::PAGE_PROSE)
                    .child(page),
            ),
        )
}

#[cfg(test)]
mod tests;
