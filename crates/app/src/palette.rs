//! The palette at the title bar's centre: one field that finds a session or a
//! project and goes to it, or, led by `>`, finds a command and runs it, as
//! its key would.
//!
//! Its list floats under the field while the field holds the keyboard, and
//! goes when the keyboard leaves. **It runs only the window's own commands**:
//! one that acts on a card or a list row would act on the title bar, where
//! the keyboard is while the palette is open.

use crate::controls::Short as _;
use crate::keymap::{COMMANDS, Command};
use crate::shell::Shell;
use crate::state::Shared;
use crate::tokens::{ROW_H, TITLE_SEARCH_W};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, AppContext as _, Context, Entity, FocusHandle, Focusable as _, InteractiveElement as _,
    IntoElement, ParentElement as _, ScrollHandle, SharedString, StatefulInteractiveElement as _,
    Styled as _, Subscription, WeakFocusHandle, Window, actions, div,
};
use gpui_component::input::{InputEvent, InputState};
use gpui_component::{ActiveTheme as _, Icon, IconName, Sizable as _, StyledExt as _, h_flex};

actions!(palette, [PaletteUp, PaletteDown]);

/// How many hits the list holds; the rest are counted under it, and a
/// longer query narrows them.
const PALETTE_CAP: usize = 50;
/// How many rows show at once; the rest scroll.
const PALETTE_ROWS: f32 = 12.;
/// What leads a query for commands rather than sessions and projects.
const COMMAND_LEAD: char = '>';

/// The field, the row the keyboard is on, and where the keyboard was before
/// the palette took it.
pub(crate) struct Palette {
    input: Entity<InputState>,
    /// Holds the field and its list: the list shows while the keyboard is
    /// anywhere in here.
    focus: FocusHandle,
    cursor: usize,
    scroll: ScrollHandle,
    /// Where the keyboard was before Ctrl+K put it here; `None` when a click
    /// did, and closing then gives it to the conversation.
    back: Option<WeakFocusHandle>,
    _input: Subscription,
    _left: Subscription,
}

impl Palette {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Shell>) -> Self {
        let input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Search sessions and projects, > for commands")
        });
        let _input = cx.subscribe_in(&input, window, |shell, _, event, window, cx| match event {
            InputEvent::Change => {
                shell.palette_mut().cursor = 0;
                shell.palette().scroll.scroll_to_item(0);
                cx.notify();
            }
            InputEvent::PressEnter { .. } => shell.palette_take(window, cx),
            InputEvent::Focus | InputEvent::Blur => cx.notify(),
        });
        let focus = cx.focus_handle();
        // Left by a click elsewhere, the palette forgets where it was opened
        // from and what was typed, so the next opening starts afresh rather
        // than giving the keyboard back to something no longer drawn.
        let _left = cx.on_focus_out(&focus, window, |shell, _, window, cx| {
            let input = shell.palette().input.clone();
            input.update(cx, |input, cx| input.set_value("", window, cx));
            let palette = shell.palette_mut();
            palette.back = None;
            palette.cursor = 0;
            cx.notify();
        });
        Self {
            input,
            focus,
            cursor: 0,
            scroll: ScrollHandle::new(),
            back: None,
            _input,
            _left,
        }
    }
}

/// What a row of the list goes to.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Pick {
    Session {
        root: usize,
        session: usize,
    },
    Project(usize),
    /// A command, by its place in the keymap.
    Command(usize),
}

/// A row of the list: what it goes to, its name, and what tells two of the
/// same name apart (a session's project, a command's key).
struct Hit {
    pick: Pick,
    name: SharedString,
    detail: Option<SharedString>,
}

/// Whether every word of `query` is in `text`, ignoring case.
fn matches(query: &str, text: &str) -> bool {
    let text = text.to_lowercase();
    query
        .split_whitespace()
        .all(|word| text.contains(&word.to_lowercase()))
}

/// The commands the palette offers: the window's own, which act wherever the
/// keyboard is, less the one that opens the palette.
fn offered(command: &Command) -> bool {
    command.context.split_whitespace().next() == Some("Shell") && command.id != "focus_rail_search"
}

/// The commands `query` (without its lead) finds, in the keymap's order.
fn find_commands(query: &str) -> impl Iterator<Item = (usize, &'static Command)> {
    COMMANDS
        .iter()
        .enumerate()
        .filter(move |(_, command)| offered(command) && matches(query, command.label))
}

/// The sessions, then the projects, `query` finds: a session by its title or
/// its project's name, a project by its name. `sessions` is each session's
/// project and place in it and its title; `projects` each project's name.
fn find_places(
    query: &str,
    sessions: &[(usize, usize, SharedString)],
    projects: &[SharedString],
) -> Vec<Hit> {
    let sessions = sessions.iter().filter_map(|(root, session, title)| {
        let project = projects.get(*root)?;
        matches(query, &format!("{title} {project}")).then(|| Hit {
            pick: Pick::Session {
                root: *root,
                session: *session,
            },
            name: title.clone(),
            detail: Some(project.clone()),
        })
    });
    let projects = projects
        .iter()
        .enumerate()
        .filter(|(_, name)| matches(query, name))
        .map(|(root, name)| Hit {
            pick: Pick::Project(root),
            name: name.clone(),
            detail: Some("Project".into()),
        });
    sessions.chain(projects).collect()
}

impl Shell {
    /// Every hit for what the field holds, uncapped.
    fn palette_hits(&self, cx: &App) -> Vec<Hit> {
        let query = self.palette().input.read(cx).value();
        if let Some(query) = query.trim_start().strip_prefix(COMMAND_LEAD) {
            let keymap = &Shared::global(cx).keymap;
            return find_commands(query)
                .map(|(at, command)| Hit {
                    pick: Pick::Command(at),
                    name: command.label.into(),
                    detail: command.keys(keymap).first().map(|key| key.into()),
                })
                .collect();
        }
        let roots = &self.workspace_window().workspace.roots;
        let projects: Vec<SharedString> =
            roots.iter().map(|root| root.label.clone().into()).collect();
        let sessions: Vec<(usize, usize, SharedString)> = roots
            .iter()
            .enumerate()
            .flat_map(|(root, project)| {
                project
                    .sessions
                    .iter()
                    .enumerate()
                    .map(move |(session, s)| {
                        let row = self.session_row(s.uid, cx);
                        let title = crate::rail::session_label(row.title.as_deref(), s.title());
                        (root, session, title)
                    })
            })
            .collect();
        find_places(&query, &sessions, &projects)
    }

    /// Put the keyboard in the palette, noting where it was so leaving the
    /// palette gives it back.
    pub(crate) fn open_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let palette = self.palette();
        if !palette.focus.contains_focused(window, cx) {
            let back = window.focused(cx).map(|focus| focus.downgrade());
            self.palette_mut().back = back;
        }
        let input = self.palette().input.clone();
        input.read(cx).focus_handle(cx).focus(window, cx);
        cx.notify();
    }

    /// Empty the field and give the keyboard back to where it was, else to
    /// the conversation.
    fn close_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let input = self.palette().input.clone();
        input.update(cx, |input, cx| input.set_value("", window, cx));
        match self
            .palette_mut()
            .back
            .take()
            .and_then(|back| back.upgrade())
        {
            Some(focus) => focus.focus(window, cx),
            None => self
                .chat_pane()
                .clone()
                .update(cx, |pane, cx| pane.reclaim_focus(window, cx)),
        }
        cx.notify();
    }

    /// Move the keyboard's row by `step`, held to the list.
    fn palette_step(&mut self, step: isize, cx: &mut Context<Self>) {
        let shown = self.palette_hits(cx).len().min(PALETTE_CAP);
        let palette = self.palette_mut();
        palette.cursor = palette
            .cursor
            .saturating_add_signed(step)
            .min(shown.saturating_sub(1));
        palette.scroll.scroll_to_item(palette.cursor);
        cx.notify();
    }

    /// Go to the keyboard's row.
    fn palette_take(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let cursor = self.palette().cursor;
        if let Some(hit) = self.palette_hits(cx).into_iter().nth(cursor) {
            self.palette_go(hit.pick, window, cx);
        }
    }

    /// Go where `pick` says. A command runs where the keyboard was, as its
    /// key would have.
    fn palette_go(&mut self, pick: Pick, window: &mut Window, cx: &mut Context<Self>) {
        self.close_palette(window, cx);
        match pick {
            Pick::Session { root, session } => self.select_root_session(root, session, window, cx),
            Pick::Project(root) => self.select_root(root, window, cx),
            Pick::Command(at) => window.dispatch_action(COMMANDS[at].action(), cx),
        }
    }
}

/// The field at the title bar's centre, its list under it while it holds the
/// keyboard. The title bar drags the window from a press and opens the
/// window's menu from a right press, so either stops here before the bar.
pub(crate) fn palette_field(
    shell: &Shell,
    window: &Window,
    cx: &mut Context<Shell>,
) -> impl IntoElement + use<> {
    let palette = shell.palette();
    let open = palette.focus.contains_focused(window, cx);
    // No search mark, and its words a step under the bar's controls, at the
    // regular weight: the bar's quietest thing, until it is used.
    let field = onehand_ui::text_input(&palette.input, cx)
        .cleanable(true)
        .short()
        .text_xs()
        .font_weight(gpui::FontWeight::NORMAL)
        // Its edge alone parts it from the bar: no fill of its own, so it
        // stands on the window's fill as the bar does.
        .bg(gpui::transparent_black())
        .when_some(
            crate::keymap::first_key("focus_rail_search", cx),
            |input, key| input.suffix(gpui_component::kbd::Kbd::new(key)),
        );
    div()
        .v_flex()
        .min_w_0()
        .w(TITLE_SEARCH_W)
        .key_context("Palette")
        .track_focus(&palette.focus)
        .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_mouse_down(gpui::MouseButton::Right, |_, _, cx| cx.stop_propagation())
        .on_action(
            cx.listener(|shell: &mut Shell, _: &PaletteUp, _, cx| shell.palette_step(-1, cx)),
        )
        .on_action(
            cx.listener(|shell: &mut Shell, _: &PaletteDown, _, cx| shell.palette_step(1, cx)),
        )
        .on_action(cx.listener(
            |shell: &mut Shell, _: &gpui_component::input::Escape, window, cx| {
                shell.close_palette(window, cx)
            },
        ))
        .child(field)
        .when(open, |el| el.child(list(shell, cx)))
}

/// The hits, capped, floated under the field at its width, with how many the
/// cap left out under them.
fn list(shell: &Shell, cx: &mut Context<Shell>) -> impl IntoElement + use<> {
    let palette = shell.palette();
    let hits = shell.palette_hits(cx);
    let total = hits.len();
    let cursor = palette.cursor;
    let commands = palette
        .input
        .read(cx)
        .value()
        .trim_start()
        .starts_with(COMMAND_LEAD);
    let rows: Vec<_> = hits
        .into_iter()
        .take(PALETTE_CAP)
        .enumerate()
        .map(|(i, hit)| row(i, hit, i == cursor, cx))
        .collect();
    let muted = cx.theme().muted_foreground;
    let note = match (total, commands) {
        (0, true) => Some("No command matches".to_string()),
        (0, false) => Some("No session or project matches".to_string()),
        (n, _) if n > PALETTE_CAP => {
            Some(format!("Showing {PALETTE_CAP} of {n}; type more to narrow"))
        }
        _ => None,
    };
    let surface = crate::chat::composer::popup_surface(cx)
        .mt_1()
        .child(
            div()
                .id("palette-list")
                .max_h(gpui::rems(ROW_H.0 * PALETTE_ROWS))
                .overflow_y_scroll()
                .track_scroll(&palette.scroll)
                .children(rows),
        )
        .children(note.map(|note| {
            h_flex()
                .h(ROW_H)
                .px_2()
                .text_xs()
                .text_color(muted)
                .child(note)
        }));
    // The float is placed out of the flow, at the top of what holds it, so it
    // hangs from an empty line under the field rather than covering it.
    div().relative().w_full().child(
        gpui::deferred(
            gpui::anchored()
                .snap_to_window()
                .child(div().w(TITLE_SEARCH_W).occlude().child(surface)),
        )
        .with_priority(1),
    )
}

/// One hit: its mark, its name, and on the right its detail, a command's key
/// as the key's hint is drawn.
fn row(i: usize, hit: Hit, highlighted: bool, cx: &mut Context<Shell>) -> impl IntoElement + use<> {
    let mark = match hit.pick {
        Pick::Session { .. } => Some(IconName::Bot),
        Pick::Project(_) => Some(IconName::Folder),
        Pick::Command(_) => None,
    };
    let detail = hit.detail.map(|detail| match hit.pick {
        Pick::Command(_) => gpui::Keystroke::parse(&detail)
            .map(|key| gpui_component::kbd::Kbd::new(key).into_any_element())
            .unwrap_or_else(|_| detail.into_any_element()),
        _ => div()
            .min_w_0()
            .truncate()
            .text_xs()
            .text_color(crate::theme::meta_ink(cx))
            .child(detail)
            .into_any_element(),
    });
    let pick = hit.pick;
    crate::chat::composer::popup_row(("palette-row", i), highlighted, cx)
        .h(ROW_H)
        .children(mark.map(|mark| onehand_ui::mark_slot(Icon::new(mark).small())))
        .child(
            div()
                .min_w_0()
                .truncate()
                .text_size(crate::tokens::CHROME_TEXT)
                .child(hit.name),
        )
        .child(div().flex_1())
        .children(detail)
        .on_click(
            cx.listener(move |shell: &mut Shell, _, window, cx| shell.palette_go(pick, window, cx)),
        )
}

#[cfg(test)]
mod tests {
    use super::{Pick, find_commands, find_places, matches};

    #[test]
    fn every_word_of_the_query_is_found_ignoring_case() {
        assert!(matches("  RETRY  api", "Retry the build atlas-api"));
        assert!(matches("", "anything"));
        assert!(!matches("retry billing", "Retry the build atlas-api"));
    }

    #[test]
    fn sessions_come_before_projects_and_match_their_project_too() {
        let projects = ["atlas-api".into(), "atlas-billing".into()];
        let sessions = [(0, 0, "Fix login".into()), (1, 0, "Invoices".into())];
        let picks: Vec<Pick> = find_places("billing", &sessions, &projects)
            .into_iter()
            .map(|hit| hit.pick)
            .collect();
        assert_eq!(
            picks,
            vec![
                Pick::Session {
                    root: 1,
                    session: 0
                },
                Pick::Project(1)
            ]
        );
    }

    #[test]
    fn commands_are_the_windows_own_less_the_palettes() {
        let ids: Vec<&str> = find_commands("").map(|(_, command)| command.id).collect();
        assert!(ids.contains(&"settings"));
        assert!(!ids.contains(&"focus_rail_search"));
        // A card's command would act on the title bar, where the keys are.
        assert!(!ids.contains(&"question_take"));
        assert!(!ids.contains(&"rail_down"));
        // Nor the terminal's shell list, whose context only starts alike.
        assert!(!ids.contains(&"shell_row_close"));
        let found: Vec<&str> = find_commands("terminal").map(|(_, c)| c.id).collect();
        assert_eq!(found, vec!["terminal"]);
    }
}
