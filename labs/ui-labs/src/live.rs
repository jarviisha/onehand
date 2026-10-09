//! The composer in the chat, working: a real field, the `+` menu, `@` and `/`
//! completion, model and effort, Fast, the branch and mode menus, the
//! attachment tray, and a turn that runs, can be stopped, and can have prompts
//! queued behind it. The agent's answers are canned.
use super::composer::Act;
use super::*;
use gpui::{AnyElement, ElementId, Entity, ScrollHandle, Subscription, Task};
use gpui_component::Disableable as _;
use gpui_component::input::{Escape, InputEvent, Textarea, TextareaState};
use std::{rc::Rc, time::Duration};

pub(super) const MODELS: [(&str, &str); 3] = [
    ("Sonnet 5", "fast, most tasks"),
    ("Opus 5.5", "slower, hardest tasks"),
    ("Haiku 4.5", "quickest"),
];
pub(super) const EFFORTS: [&str; 3] = ["Low", "Medium", "High"];
pub(super) const MODES: [(&str, &str); 4] = [
    ("Ask before edits", "every edit and command asks"),
    ("Accept edits", "commands still ask"),
    ("Plan only", "reads, never writes"),
    ("Bypass permissions", "nothing asks"),
];
pub(super) const BRANCHES: [(&str, &str); 3] = [
    ("main", "3 changes"),
    ("fix/retry-clock", "2 ahead"),
    ("feat/charts", ""),
];
pub(super) const FILES: [(&str, &str); 8] = [
    ("src/backoff.rs", "modified"),
    ("tests/flaky.rs", "modified"),
    ("src/retry.rs", ""),
    ("src/retry/policy.rs", ""),
    ("benches/retry.rs", ""),
    ("docs/retry.md", ""),
    ("README.md", ""),
    ("Cargo.toml", ""),
];
pub(super) const COMMANDS: [(&str, &str); 5] = [
    ("/review", "Review the working tree"),
    ("/compact", "Summarise the conversation so far"),
    ("/init", "Write a CLAUDE.md for this project"),
    ("/clear", "Start the conversation over"),
    ("/run-check", "Run the project's check command"),
];
pub(super) const ATTACHABLE: [(IconName, &str, &str); 3] = [
    (IconName::File, "retry.rs", "4 KB"),
    (IconName::Frame, "screenshot.png", "182 KB"),
    (IconName::File, "ci-log.txt", "51 KB"),
];
const REPLIES: [&str; 3] = [
    "Done. The backoff now reads an injected clock, and the test passes 200 runs in a row.",
    "I looked at it: nothing else in the crate reads wall-clock time, so that was the only cause.",
    "Noted. I will keep that in mind for the rest of this session.",
];
/// How long a canned turn runs before it answers.
const TURN: Duration = Duration::from_millis(2400);

/// Which popup is open over the field.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum Menu {
    Plus,
    Model,
    Mode,
    Branch,
    Mention,
    Command,
}

/// What this session added to the transcript.
pub(super) enum Said {
    /// The prompt and the indices of what was attached, into `ATTACHABLE`.
    User(String, Vec<usize>),
    Agent(&'static str),
    Notice(&'static str),
}

pub(super) struct Live {
    pub(super) input: Entity<TextareaState>,
    pub(super) open: Option<Menu>,
    /// What follows the `@` or `/` being completed.
    pub(super) query: String,
    /// The popup row Enter takes; the arrows move it.
    pub(super) highlight: usize,
    pub(super) fast: bool,
    pub(super) model: usize,
    pub(super) effort: usize,
    pub(super) mode: usize,
    pub(super) branch: usize,
    pub(super) tray: Vec<usize>,
    pub(super) queued: Vec<String>,
    pub(super) said: Vec<Said>,
    /// The running turn; dropping it is what Stop does.
    pub(super) turn: Option<Task<()>>,
    pub(super) replies: usize,
    /// The transcript's scroll, so what is sent or answered comes into view.
    pub(super) scroll: ScrollHandle,
    /// The canned conversation's last turn is still running. It never ends
    /// on its own, so the lab opens on a running chat until Stop.
    pub(super) canned_running: bool,
    pub(super) _sub: Subscription,
}

impl Live {
    pub(super) fn new(window: &mut Window, cx: &mut Context<Labs>) -> Self {
        let input = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(1, 8)
                // Enter sends (or takes a completion); Shift+Enter is a newline.
                .submit_on_enter(true)
                .placeholder(super::composer::PLACEHOLDER)
        });
        // Open on the running turn and the composer under it.
        let scroll = ScrollHandle::new();
        scroll.scroll_to_bottom();
        let _sub = cx.subscribe_in(
            &input,
            window,
            |this: &mut Labs, _, event: &InputEvent, window, cx| match event {
                InputEvent::Change => this.retrigger(cx),
                InputEvent::PressEnter { shift: false, .. } => this.enter(window, cx),
                InputEvent::Focus | InputEvent::Blur | InputEvent::PressEnter { .. } => {}
            },
        );
        Self {
            input,
            open: None,
            query: String::new(),
            highlight: 0,
            fast: false,
            model: 0,
            effort: 2,
            mode: 0,
            branch: 0,
            tray: Vec::new(),
            queued: Vec::new(),
            said: Vec::new(),
            turn: None,
            replies: 0,
            scroll,
            canned_running: true,
            _sub,
        }
    }

    /// A turn runs: one sent here, or the canned one not yet stopped.
    pub(super) fn running(&self) -> bool {
        self.turn.is_some() || self.canned_running
    }
}

/// The completion the field's text asks for: `@` at the start of its last
/// word, or `/` heading a field with no space in it yet.
fn trigger(text: &str) -> Option<(Menu, &str)> {
    let token = text.rsplit(char::is_whitespace).next().unwrap_or("");
    if let Some(q) = token.strip_prefix('@') {
        Some((Menu::Mention, q))
    } else if text.starts_with('/') && !text.contains(char::is_whitespace) {
        Some((Menu::Command, &text[1..]))
    } else {
        None
    }
}

/// The text once `pick` replaces the word being completed.
fn completed(text: &str, menu: Menu, pick: &str) -> String {
    match menu {
        Menu::Command => format!("{pick} "),
        _ => {
            let at = text.rfind('@').unwrap_or(text.len());
            format!("{}@{pick} ", &text[..at])
        }
    }
}

pub(super) fn act(f: impl Fn(&mut Labs, &mut Window, &mut Context<Labs>) + 'static) -> Act {
    Rc::new(f)
}

impl Labs {
    // ---- behaviour ---------------------------------------------------------

    pub(super) fn text(&self, cx: &App) -> String {
        self.live.input.read(cx).value().to_string()
    }

    pub(super) fn set_text(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.live
            .input
            .update(cx, |s, cx| s.set_value(text.to_string(), window, cx));
    }

    pub(super) fn focus_field(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.live.input.update(cx, |s, cx| s.focus(window, cx));
    }

    pub(super) fn toggle(&mut self, menu: Menu, cx: &mut Context<Self>) {
        self.live.highlight = 0;
        self.live.open = if self.live.open == Some(menu) {
            None
        } else {
            Some(menu)
        };
        cx.notify();
    }

    /// Open or close `@` / `/` completion from what the field now says.
    pub(super) fn retrigger(&mut self, cx: &mut Context<Self>) {
        let text = self.text(cx);
        match trigger(&text) {
            Some((menu, q)) => {
                if self.live.open != Some(menu) || self.live.query != q {
                    self.live.highlight = 0;
                }
                self.live.open = Some(menu);
                self.live.query = q.to_string();
            }
            None if matches!(self.live.open, Some(Menu::Mention | Menu::Command)) => {
                self.live.open = None;
            }
            None => {}
        }
        cx.notify();
    }

    pub(super) fn matches(&self) -> Vec<(&'static str, &'static str)> {
        let q = self.live.query.to_lowercase();
        let pool: &[(&str, &str)] = match self.live.open {
            Some(Menu::Mention) => &FILES,
            Some(Menu::Command) => &COMMANDS,
            _ => &[],
        };
        pool.iter()
            .filter(|(name, _)| name.to_lowercase().contains(&q))
            .copied()
            .collect()
    }

    /// Replace the token being completed with the candidate.
    pub(super) fn complete(&mut self, pick: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(menu) = self.live.open else { return };
        let next = completed(&self.text(cx), menu, pick);
        self.live.open = None;
        self.set_text(&next, window, cx);
        self.focus_field(window, cx);
    }

    pub(super) fn enter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // An open popup takes Enter: the highlighted row, as a click would.
        if let Some(menu) = self.live.open {
            let pick = self
                .menu_spec(menu)
                .items
                .into_iter()
                .nth(self.live.highlight)
                .and_then(|it| it.on);
            if let Some(on) = pick {
                on(self, window, cx);
            }
            return;
        }
        self.send(window, cx);
    }

    /// Move the popup's highlight, wrapping, within the rows it shows.
    pub(super) fn step_highlight(&mut self, by: isize, cx: &mut Context<Self>) {
        let Some(menu) = self.live.open else { return };
        let n = self.menu_spec(menu).items.len().min(POPUP_LIST_CAP);
        if n == 0 {
            return;
        }
        self.live.highlight = (self.live.highlight as isize + by).rem_euclid(n as isize) as usize;
        cx.notify();
    }

    /// Send the draft, or queue it behind the running turn.
    pub(super) fn send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.text(cx).trim().to_string();
        if self.live.running() {
            // Only words queue; attachments stay in the tray for the prompt
            // that is sent once the turn is over.
            if text.is_empty() {
                return;
            }
            self.live.open = None;
            self.set_text("", window, cx);
            self.live.queued.push(text);
            cx.notify();
            return;
        }
        if text.is_empty() && self.live.tray.is_empty() {
            return;
        }
        self.live.open = None;
        self.set_text("", window, cx);
        let files = std::mem::take(&mut self.live.tray);
        self.start_turn(text, files, cx);
    }

    fn start_turn(&mut self, text: String, files: Vec<usize>, cx: &mut Context<Self>) {
        self.live.said.push(Said::User(text, files));
        self.live.scroll.scroll_to_bottom();
        self.live.turn = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(TURN).await;
            this.update(cx, |this, cx| this.end_turn(cx)).ok();
        }));
        cx.notify();
    }

    fn end_turn(&mut self, cx: &mut Context<Self>) {
        let reply = REPLIES[self.live.replies % REPLIES.len()];
        self.live.replies += 1;
        self.live.said.push(Said::Agent(reply));
        self.live.scroll.scroll_to_bottom();
        self.live.turn = None;
        self.next_queued(cx);
        cx.notify();
    }

    pub(super) fn stop(&mut self, cx: &mut Context<Self>) {
        self.live.turn = None;
        self.live.canned_running = false;
        self.live
            .said
            .push(Said::Notice("Stopped. The agent did not finish this turn."));
        self.live.scroll.scroll_to_bottom();
        // Stopping holds the queue too: what was queued waits to be sent or
        // removed by hand, rather than starting the moment the person stopped.
        cx.notify();
    }

    fn next_queued(&mut self, cx: &mut Context<Self>) {
        if !self.live.queued.is_empty() {
            let text = self.live.queued.remove(0);
            self.start_turn(text, Vec::new(), cx);
        }
    }

    // ---- drawing -----------------------------------------------------------

    /// What this session added, below the canned transcript.
    pub(super) fn said(&self, p: &Palette, window: &Window, cx: &App) -> Vec<AnyElement> {
        let mut out: Vec<AnyElement> = self
            .live
            .said
            .iter()
            .enumerate()
            .map(|(i, s)| match s {
                Said::User(text, files) => {
                    let names: Vec<&str> = files.iter().map(|&i| ATTACHABLE[i].1).collect();
                    let text = if text.is_empty() {
                        "(attachments only)".to_string()
                    } else {
                        text.clone()
                    };
                    self.bubble(p, text, &names, cx).into_any_element()
                }
                Said::Agent(text) => self
                    .md(
                        p,
                        ElementId::NamedInteger("said".into(), i as u64),
                        *text,
                        window,
                        cx,
                    )
                    .into_any_element(),
                Said::Notice(text) => self.notice(p, text).into_any_element(),
            })
            .collect();
        // The canned turn draws its own running blocks; this line is only
        // for a turn sent here.
        if self.live.turn.is_some() {
            out.push(
                h_flex()
                    .gap_2()
                    .text_size(self.read(TEXT_READ_SM))
                    .text_color(p.accent)
                    .child(gpui_component::spinner::Spinner::new().small())
                    .child("Working…")
                    .into_any_element(),
            );
        }
        out
    }

    /// The composer stack: queued prompts, then the card, with its popup
    /// floating above it.
    pub(super) fn live_composer(
        &self,
        p: &Palette,
        narrow: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let mono = cx.theme().mono_font_family.clone();
        let live = &self.live;
        let running = live.running();
        let typed = !self.text(cx).trim().is_empty();

        let send: AnyElement = if running {
            h_flex()
                .gap_1()
                .when(typed, |d| {
                    d.child(
                        action("queue")
                            .outline()
                            .small()
                            .label("Queue")
                            .on_click(cx.listener(|this, _, w, cx| this.send(w, cx))),
                    )
                })
                .child(
                    action("stop")
                        .danger()
                        .small()
                        .label("Stop")
                        .on_click(cx.listener(|this, _, _, cx| this.stop(cx))),
                )
                .into_any_element()
        } else {
            action("send")
                .primary()
                .small()
                .icon(IconName::ArrowUp)
                .tooltip("Send")
                // Spent until there is something to send, so a filled arrow
                // means Enter will do something.
                .disabled(!typed && live.tray.is_empty())
                .on_click(cx.listener(|this, _, w, cx| this.send(w, cx)))
                .into_any_element()
        };

        let tray = (!live.tray.is_empty()).then(|| {
            h_flex()
                .flex_wrap()
                .gap_1()
                .children(live.tray.iter().enumerate().map(|(slot, &i)| {
                    let (icon, name, size) = ATTACHABLE[i].clone();
                    h_flex()
                        .h_6()
                        .pl_2()
                        .pr_1()
                        .gap_2()
                        .rounded(cx.theme().radius)
                        .border_1()
                        .border_color(p.hairline)
                        .bg(p.sunken)
                        .text_xs()
                        .child(Icon::new(icon).xsmall().text_color(p.muted))
                        .child(div().text_color(p.text).child(name))
                        .child(div().text_color(p.muted).child(size))
                        .child(
                            action(("drop-attachment", slot))
                                .ghost()
                                .xsmall()
                                .icon(IconName::Close)
                                .tooltip("Remove")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.live.tray.remove(slot);
                                    cx.notify();
                                })),
                        )
                }))
        });

        let queued = live.queued.iter().enumerate().map(|(i, text)| {
            h_flex()
                .w_full()
                .px_3()
                .py_2()
                .gap_2()
                .child(
                    div()
                        .text_xs()
                        .text_color(p.muted)
                        .font_medium()
                        .child("Queued"),
                )
                .child(div().flex_1().min_w_0().truncate().child(text.clone()))
                .child(
                    action(("queued-edit", i))
                        .ghost()
                        .small()
                        .label("Edit")
                        .on_click(cx.listener(move |this, _, w, cx| {
                            let text = this.live.queued.remove(i);
                            this.set_text(&text, w, cx);
                            this.focus_field(w, cx);
                            cx.notify();
                        })),
                )
                .child(
                    action(("queued-drop", i))
                        .ghost()
                        .small()
                        .icon(IconName::Close)
                        .tooltip("Remove from the queue")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.live.queued.remove(i);
                            cx.notify();
                        })),
                )
        });
        let queued: Vec<_> = queued.collect();

        let (model, _) = MODELS[live.model];
        let (branch, changes) = BRANCHES[live.branch];
        let branch_label = if changes.is_empty() {
            branch.to_string()
        } else {
            format!("{branch} · {changes}")
        };
        let is_open = |menu: Menu| live.open == Some(menu);

        let card = super::composer::card(p, cx)
            .children(tray)
            .child(
                // The whole field area takes the caret, not only its first line.
                div()
                    .id("field")
                    .min_h_10()
                    .cursor_text()
                    .text_size(self.read(TEXT_READ))
                    .line_height(self.read(TEXT_READ * LEADING_READ))
                    .on_click(cx.listener(|this, _, w, cx| this.focus_field(w, cx)))
                    .child(Textarea::new(&live.input).appearance(false)),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        super::composer::plus_chip(is_open(Menu::Plus))
                            .on_click(cx.listener(|this, _, _, cx| this.toggle(Menu::Plus, cx))),
                    )
                    .child(
                        super::composer::fast_chip("fast", live.fast, p).on_click(cx.listener(
                            |this, _, _, cx| {
                                this.live.fast = !this.live.fast;
                                cx.notify();
                            },
                        )),
                    )
                    .child(
                        super::composer::model_chip(
                            p,
                            model,
                            EFFORTS[live.effort],
                            is_open(Menu::Model),
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.toggle(Menu::Model, cx))),
                    )
                    .child(div().flex_1())
                    .child(send),
            );

        // Below `COMPOSER_SPLIT` the branch and the mode take a line each,
        // rather than one squeezing the other.
        let branch = super::composer::strip_chip(
            "branch",
            p,
            crate::assets::GIT_BRANCH,
            branch_label,
            is_open(Menu::Branch),
        )
        .on_click(cx.listener(|this, _, _, cx| this.toggle(Menu::Branch, cx)));
        let mode = super::composer::strip_chip(
            "mode",
            p,
            crate::assets::SHIELD,
            MODES[live.mode].0,
            is_open(Menu::Mode),
        )
        .on_click(cx.listener(|this, _, _, cx| this.toggle(Menu::Mode, cx)));
        let strip = if narrow {
            v_flex().items_start()
        } else {
            h_flex().justify_between().gap_2()
        }
        .px_1p5()
        .pt_1p5()
        .child(branch)
        .child(mode);

        let popup = live.open.map(|menu| self.live_popup(menu, p, mono, cx));

        v_flex()
            .w_full()
            .gap_2p5()
            .children(queued)
            .child(v_flex().relative().child(card).child(strip).children(popup))
            // While a popup is open the arrows walk it instead of the caret.
            .when(live.open.is_some(), |d| d.key_context("LabsPopup"))
            .on_action(cx.listener(|this, _: &PopupUp, _, cx| this.step_highlight(-1, cx)))
            .on_action(cx.listener(|this, _: &PopupDown, _, cx| this.step_highlight(1, cx)))
            .on_action(cx.listener(|this, _: &Escape, _, cx| {
                if this.live.open.take().is_some() {
                    cx.notify();
                } else {
                    cx.propagate();
                }
            }))
            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                if this.live.open.take().is_some() {
                    cx.notify();
                }
            }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completion_opens_on_its_trigger_and_replaces_only_its_word() {
        assert!(matches!(
            trigger("look at @ret"),
            Some((Menu::Mention, "ret"))
        ));
        assert!(matches!(trigger("@"), Some((Menu::Mention, ""))));
        assert!(matches!(trigger("/rev"), Some((Menu::Command, "rev"))));
        // A finished word, a slash mid-sentence, a mail address: nothing.
        assert!(trigger("look at @src/retry.rs ").is_none());
        assert!(trigger("/review now").is_none());
        assert!(trigger("and/or").is_none());
        assert!(trigger("me@host").is_none());

        assert_eq!(
            completed("look at @ret", Menu::Mention, "src/retry.rs"),
            "look at @src/retry.rs "
        );
        assert_eq!(completed("/rev", Menu::Command, "/review"), "/review ");
    }
}
