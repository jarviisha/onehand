//! The bottom terminal panel: the user's own shell.
//!
//! Distinct from the ACP terminals the agent runs — those are a byte stream the
//! transcript renders (`chat::transcript`) and need no PTY widget at all. This
//! is the one the user types into.
//!
//! Scope is **per project root**: each root owns its tab set, and switching
//! roots swaps the whole thing — a shell belongs to a project, not to a window.
//!
//! A tab runs one of two things. The user's login shell, which is what the `+`
//! and the terminal key open — and Neovim, on the project root, which is what
//! `Ctrl+Shift+N` opens. There is no third kind of panel for the editor: it is a
//! program in a PTY like any other, and the grid underneath now carries what
//! such a program needs of a terminal — mouse reporting with the `Shift` bypass
//! that takes selection back, the cursor shapes `DECSCUSR` asks for, `OSC 52` so
//! a yank reaches the system clipboard, and answers to the queries it sends at
//! startup.
//!
//! Neovim gets its own tab rather than a second one every press: the point of
//! the key is to reach the editor, and a key that opened another editor each
//! time it was pressed would be a key nobody could use to come back to their
//! work.

use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, AppContext, Context, Entity, EventEmitter, FocusHandle, Focusable, InteractiveElement,
    IntoElement, ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, Window,
    div, px,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::dock::{Panel, PanelControl, PanelEvent};
use gpui_component::{ActiveTheme, Icon, IconName, Sizable as _, StyledExt};
use gpui_terminal::{ColorPalette, TerminalConfig, TerminalView};
use portable_pty::{CommandBuilder, NativePtySystem, PtySize, PtySystem};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// Scrollback kept per terminal: history is bytes in RAM per tab, and nobody
/// scrolls back further than this without reaching for the shell's own pager.
const SCROLLBACK: usize = 2000;

/// What a tab was started to run.
///
/// Not cosmetic: it is how the panel finds an editor tab that is already open,
/// which is what stops `Ctrl+Shift+N` opening a second Neovim on top of the
/// first. Matching on the label would work until somebody's shell was called
/// `nvim`, or until a program set the tab's title.
#[derive(Clone, Copy, PartialEq)]
enum Program {
    /// The user's login shell.
    Shell,
    /// Neovim, opened on the project root.
    Editor,
}

/// One shell tab.
struct Shell {
    view: Entity<TerminalView>,
    label: SharedString,
    program: Program,
    /// Kept alive for the tab's life. Dropping the PTY pair hangs up the
    /// terminal, which is what gets a well-behaved shell to leave; [`Drop`]
    /// below is what handles the rest.
    ///
    /// Shared with the view's resize callback, which is why it is behind an
    /// `Arc<Mutex<_>>` rather than owned outright -- see [`spawn_shell`]. The
    /// callback is `Send + Sync`, and a `Mutex` is what makes a `MasterPty`
    /// that is only `Send` usable from one.
    _pty: Arc<Mutex<Box<dyn portable_pty::MasterPty + Send>>>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
}

/// Close the tab's child for real.
///
/// Dropping the PTY alone was the whole shutdown, and it is not enough twice
/// over: a child that ignores `SIGHUP` (`nohup`, anything that called `setsid`)
/// keeps running with no tab left to show it, and a child that *did* exit stays
/// a zombie until the process ends, because nobody reaps it.
///
/// `kill` then `wait`, in that order and unconditionally -- `kill` on an
/// already-dead child is a no-op, and `wait` is what actually collects it. The
/// ACP side (`onehand_core::acp::terminal`) has always done both; this is the
/// user-facing terminal catching up.
impl Drop for Shell {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The tab set one project root has open.
#[derive(Default)]
struct RootShells {
    tabs: Vec<Shell>,
    active: usize,
}

/// Theme inputs that affect the terminal palette.
///
/// Kept separately because `ColorPalette` intentionally hides its ANSI table.
/// Comparing these lets render update live terminals once per appearance
/// change instead of rewriting their config on every frame.
#[derive(Clone, Copy, PartialEq)]
struct TerminalThemeKey {
    dark: bool,
    colors: [gpui::Hsla; 17],
}

impl TerminalThemeKey {
    fn current(cx: &App) -> Self {
        let theme = cx.theme();
        Self {
            dark: theme.mode.is_dark(),
            colors: [
                theme.background,
                theme.foreground,
                theme.muted_foreground,
                theme.caret,
                theme.red,
                theme.red_light,
                theme.green,
                theme.green_light,
                theme.yellow,
                theme.yellow_light,
                theme.blue,
                theme.blue_light,
                theme.magenta,
                theme.magenta_light,
                theme.cyan,
                theme.cyan_light,
                theme.border,
            ],
        }
    }
}

pub struct TerminalPanel {
    focus_handle: FocusHandle,
    root: Option<PathBuf>,
    shells: HashMap<PathBuf, RootShells>,
    /// Surfaced when a shell cannot be started at all.
    status: Option<String>,
    /// The grid's reading size. Not a rem scale like the other panels: a
    /// terminal is a measured glyph grid, so the zoom is a font size, and
    /// changing it re-measures the cell and resizes the PTY.
    zoom: crate::zoom::Zoom,
    terminal_theme: TerminalThemeKey,
}

impl TerminalPanel {
    pub fn new(cx: &mut App) -> Entity<Self> {
        cx.new(|cx| Self {
            focus_handle: cx.focus_handle(),
            root: None,
            shells: HashMap::new(),
            status: None,
            zoom: crate::zoom::Zoom::default(),
            terminal_theme: TerminalThemeKey::current(cx),
        })
    }

    pub fn set_root(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        self.root = Some(root);
        cx.notify();
    }

    /// Open a shell on the active root.
    ///
    /// Always a new one, unlike [`Self::open_editor`]: shells are what somebody
    /// opens several of on purpose, one per thing they are watching.
    pub fn open_shell(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_tab(Program::Shell, window, cx);
    }

    /// Show the project's Neovim, opening it if it is not already running.
    ///
    /// Reaching for the editor and reaching for *another* editor are different
    /// requests, and only the first has a key: pressing this twice has to land
    /// back in the work, not beside it. A second one is still available the way
    /// every other tab is, through the `+`.
    ///
    /// Per project root, like everything else in this panel, because the editor
    /// is opened on that root and an editor rooted somewhere else is not the one
    /// the key was pressed for.
    pub fn open_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let existing = self.active_set().and_then(|set| {
            set.tabs
                .iter()
                .position(|tab| tab.program == Program::Editor)
        });
        match existing {
            Some(idx) => self.select_tab(idx, cx),
            None => self.open_tab(Program::Editor, window, cx),
        }
    }

    /// Spawn a tab on the active root and make it the one on screen.
    ///
    /// Spawned lazily and never at boot: a workspace with a dozen roots must
    /// not start a dozen shells nobody asked for.
    fn open_tab(&mut self, program: Program, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            return;
        };

        match spawn_shell(&root, program, self.zoom, window, cx) {
            Ok(shell) => {
                let set = self.shells.entry(root).or_default();
                set.tabs.push(shell);
                set.active = set.tabs.len() - 1;
                self.status = None;
            }
            Err(e) => self.status = Some(e),
        }
        cx.notify();
    }

    pub fn close_tab(&mut self, idx: usize, cx: &mut Context<Self>) {
        if let Some(set) = self.root.as_ref().and_then(|r| self.shells.get_mut(r))
            && idx < set.tabs.len()
        {
            // Dropping the tab drops its PTY, which ends the child.
            set.tabs.remove(idx);
            set.active = set.active.min(set.tabs.len().saturating_sub(1));
        }
        cx.notify();
    }

    pub fn select_tab(&mut self, idx: usize, cx: &mut Context<Self>) {
        if let Some(set) = self.root.as_ref().and_then(|r| self.shells.get_mut(r))
            && idx < set.tabs.len()
        {
            set.active = idx;
        }
        cx.notify();
    }

    /// Drop every shell this panel holds for `root`.
    ///
    /// Dropping the tabs drops their PTYs, which is what ends the children --
    /// removing a project must not leave its shells running with nothing on
    /// screen pointing at them.
    pub fn forget_root(&mut self, root: &PathBuf, cx: &mut Context<Self>) {
        self.shells.remove(root);
        if self.root.as_ref() == Some(root) {
            self.root = None;
        }
        cx.notify();
    }

    /// Step the grid's font size, re-measuring every live shell.
    ///
    /// Applied to the whole panel, not just the active tab: the tabs are one
    /// terminal in the user's head, and switching tabs must not switch size.
    /// Newly spawned shells pick the factor up in [`Self::open_shell`].
    pub fn set_zoom(&mut self, zoom: crate::zoom::Zoom, cx: &mut Context<Self>) {
        self.zoom = zoom;
        let size = crate::zoom::term_font_size(zoom);
        for set in self.shells.values() {
            for tab in &set.tabs {
                tab.view.update(cx, |view, cx| {
                    let config = TerminalConfig {
                        font_size: size,
                        ..view.config().clone()
                    };
                    view.update_config(config, cx);
                });
            }
        }
        cx.notify();
    }

    pub fn zoom(&self) -> crate::zoom::Zoom {
        self.zoom
    }

    /// Recolour every live grid after an app appearance change.
    fn sync_theme(&mut self, cx: &mut Context<Self>) {
        let current = TerminalThemeKey::current(cx);
        if self.terminal_theme == current {
            return;
        }
        self.terminal_theme = current;
        let colors = terminal_palette(cx);
        for set in self.shells.values() {
            for tab in &set.tabs {
                tab.view.update(cx, |view, cx| {
                    let config = TerminalConfig {
                        colors: colors.clone(),
                        ..view.config().clone()
                    };
                    view.update_config(config, cx);
                });
            }
        }
    }

    /// Whether the active root already has a shell open, so a shortcut that
    /// reveals the panel can spawn the first one without spawning a second on
    /// every later press.
    pub fn has_shell(&self) -> bool {
        self.active_set().is_some_and(|set| !set.tabs.is_empty())
    }

    /// Focus the active shell, not the panel: the point of reaching for the
    /// terminal is to type into it.
    pub fn focus_active(&self, window: &mut Window, cx: &mut App) {
        let handle = self
            .active_set()
            .and_then(|set| set.tabs.get(set.active))
            .map(|tab| tab.view.read(cx).focus_handle().clone());
        match handle {
            Some(handle) => handle.focus(window, cx),
            None => self.focus_handle.focus(window, cx),
        }
    }

    fn active_set(&self) -> Option<&RootShells> {
        self.root.as_ref().and_then(|r| self.shells.get(r))
    }
}

/// The family name to hand the grid.
///
/// The vendored view's own default is the literal string `monospace`, which is
/// a CSS generic and not a family any font enumeration on this machine offers.
/// Asking for a family that does not resolve fails **silently** -- the text
/// draws in whatever face the fallback picks -- and the grid is measured from a
/// shaped glyph, so the miss does not merely change the typeface: the cell is
/// sized from one font while the row is drawn in another, and every column
/// lands short of or past its glyph. The boot scan already resolved a family
/// against what is installed, so take that one.
fn mono_family(cx: &App) -> String {
    cx.theme().mono_font_family.to_string()
}

/// The program a tab runs, and the name its tab carries.
///
/// Neovim is looked up on `PATH` here rather than handed to the PTY to fail on,
/// because the two failures do not read alike: a spawn that fails comes back as
/// "No such file or directory" with no subject, and the one thing the user needs
/// told is *which* file. It is a plain `nvim` and not `$EDITOR` -- this is the
/// Neovim key, and honouring `$EDITOR` would make it open `vi` or `nano` for
/// someone who set that variable years ago for `git commit`.
fn program_command(program: Program) -> Result<(CommandBuilder, SharedString), String> {
    match program {
        Program::Shell => {
            let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
            let label = SharedString::from(
                std::path::Path::new(&shell)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| shell.clone()),
            );
            Ok((CommandBuilder::new(&shell), label))
        }
        Program::Editor => {
            let found = std::env::var_os("PATH")
                .into_iter()
                .flat_map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
                .map(|dir| dir.join("nvim"))
                .find(|candidate| candidate.is_file())
                .ok_or_else(|| "Neovim is not installed, or `nvim` is not on PATH".to_string())?;
            Ok((CommandBuilder::new(found), SharedString::from("nvim")))
        }
    }
}

/// Start `program` in `cwd`.
fn spawn_shell(
    cwd: &PathBuf,
    program: Program,
    zoom: crate::zoom::Zoom,
    window: &mut Window,
    cx: &mut App,
) -> Result<Shell, String> {
    let config = TerminalConfig {
        scrollback: SCROLLBACK,
        font_size: crate::zoom::term_font_size(zoom),
        colors: terminal_palette(cx),
        font_family: mono_family(cx),
        ..TerminalConfig::default()
    };

    // Opened at the config's own grid size so the PTY and the client-side grid
    // start out agreeing. Neither number survives the first paint -- that is
    // where the real geometry is measured and pushed through the resize
    // callback below -- but starting them apart means the shell prints its
    // prompt at one width and is told another a frame later.
    let pty = NativePtySystem::default()
        .openpty(PtySize {
            rows: config.rows as u16,
            cols: config.cols as u16,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| e.to_string())?;

    let (mut command, label) = program_command(program)?;
    command.cwd(cwd);
    // `alacritty_terminal` ships a `tty::setup_env` it never calls, and an
    // inherited foreign `TERM` breaks key and colour detection in anything
    // curses-based.
    command.env("TERM", "xterm-256color");
    command.env("COLORTERM", "truecolor");

    let child = pty
        .slave
        .spawn_command(command)
        .map_err(|e| e.to_string())?;
    let writer = pty.master.take_writer().map_err(|e| e.to_string())?;
    let reader = pty.master.try_clone_reader().map_err(|e| e.to_string())?;

    // The one thing the vendored view cannot do for itself.
    //
    // `TerminalView` re-measures the grid on every paint and resizes its own
    // `Term` when the cell count changes -- but the *PTY* only learns about it
    // through this callback, and upstream leaves it unset. Without it the
    // child's winsize stays at whatever `openpty` was given for the life of
    // the tab: `$COLUMNS`/`$LINES` never move, and every full-screen program
    // (`less`, `htop`, `vim`, `git log`'s pager) draws an 80x24 box inside a
    // panel of some other size. The client-side grid resizing correctly is
    // what makes it look like a rendering bug rather than a missing ioctl.
    //
    // This also covers zoom for free: a font-size change re-measures the cell,
    // which changes the cell count, which comes back through here.
    let master = Arc::new(Mutex::new(pty.master));
    let view = cx.new(|cx| {
        TerminalView::new(writer, reader, config, cx)
            .with_resize_callback({
                let master = master.clone();
                move |cols, rows| {
                    if let Ok(master) = master.lock() {
                        // A failed resize is not worth tearing the tab down for:
                        // the grid has already resized, so the panel stays usable
                        // and only the child's idea of its size is stale.
                        let _ = master.resize(PtySize {
                            rows: rows as u16,
                            cols: cols as u16,
                            pixel_width: 0,
                            pixel_height: 0,
                        });
                    }
                }
            })
            // The other thing the grid cannot do for itself, and the app is the
            // right half to do it: the clipboard belongs to the desktop session,
            // not to one tab.
            //
            // This is a program in the shell asking for text to be copied. It is
            // the only way a copy inside a full-screen editor reaches anything
            // outside the terminal -- the editor owns the whole screen, so the
            // selection it is copying is its own idea and there is nothing on
            // the grid for a mouse drag to take. Without this, yanking to the
            // system clipboard silently does nothing at all, which reads as the
            // editor being misconfigured.
            //
            // Failures are dropped: the clipboard can be held by another
            // process, and a tab that started printing errors because a copy did
            // not land would be worse than the copy not landing.
            .with_clipboard_store_callback(|_, _, text| {
                if let Ok(mut clipboard) = gpui_terminal::Clipboard::new() {
                    let _ = clipboard.copy(text);
                }
            })
    });
    let _ = window;

    Ok(Shell {
        view,
        label,
        program,
        _pty: master,
        child,
    })
}

impl Panel for TerminalPanel {
    fn panel_name(&self) -> &'static str {
        "Terminal"
    }

    /// No content-only maximize, because there is no tab bar to put the button
    /// on: this panel is mounted bare, the way the conversation is. The app
    /// direction still has its key.
    fn zoomable(&self, _: &App) -> Option<PanelControl> {
        None
    }

    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        SharedString::from("Terminal")
    }
}

impl EventEmitter<PanelEvent> for TerminalPanel {}

impl Focusable for TerminalPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for TerminalPanel {
    /// The panel's key context. This is the one place the app deliberately
    /// steps *out* of the way: `Ctrl+S` is bound `Shell && !Terminal`, so a
    /// program in the PTY keeps it.
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_theme(cx);
        div()
            .size_full()
            // Tracked here because this panel is mounted bare. A `TabPanel` calls
            // `track_focus` on the panel it holds, which is what normally puts
            // that handle in the focus tree; without a tab group nothing does,
            // and `contains_focused` would answer "no" however deep in the grid
            // the caret is -- silently pointing the three-state panel keymap and
            // every "which panel is this" question at some other panel.
            .track_focus(&self.focus_handle)
            .key_context("Terminal")
            .child(self.body(window, cx))
    }
}

impl TerminalPanel {
    fn body(&mut self, _: &mut Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        // Whether there *is* a root is a question about the selection, not
        // about the shell map: a root only gains an entry there once a shell
        // has been spawned in it. Reading the map instead would tell a project
        // that has simply never had a terminal opened -- which is every project
        // on the first frame after a restored layout reopens this dock -- that
        // it is not a project at all, and hide the button that would start one.
        let Some(set) = self.active_set().filter(|set| !set.tabs.is_empty()) else {
            if self.root.is_none() {
                return hint("No project root", cx).into_any_element();
            }
            return div()
                .size_full()
                .v_flex()
                .items_center()
                .justify_center()
                .gap_2()
                .child(
                    crate::controls::action("new-shell")
                        .primary()
                        .icon(Icon::new(IconName::SquareTerminal))
                        .label("New terminal")
                        .on_click(cx.listener(|panel: &mut Self, _, window, cx| {
                            panel.open_shell(window, cx);
                        })),
                )
                .children(self.status.clone().map(|status| {
                    div()
                        .text_xs()
                        .text_color(crate::theme::status_ink(cx).danger)
                        .child(status)
                        .into_any_element()
                }))
                .into_any_element();
        };

        let active = set.active;
        let body = set.tabs.get(active).map(|tab| tab.view.clone());
        let labels: Vec<SharedString> = set.tabs.iter().map(|t| t.label.clone()).collect();

        div()
            .size_full()
            .v_flex()
            .child(
                div()
                    .id("terminal-tabs")
                    .h_flex()
                    .items_center()
                    .gap_1()
                    .w_full()
                    .px_2()
                    .py_1()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .children(labels.into_iter().enumerate().map(|(i, label)| {
                        div()
                            .id(("terminal-tab", i))
                            .h_flex()
                            .items_center()
                            .gap_1()
                            .flex_none()
                            .px_2()
                            .py_0p5()
                            .rounded(cx.theme().radius)
                            .text_xs()
                            .cursor_pointer()
                            .when(i == active, |tab| {
                                tab.bg(cx.theme().accent)
                                    .text_color(cx.theme().accent_foreground)
                            })
                            .child(div().max_w(px(120.)).truncate().child(label))
                            .on_click(cx.listener(move |panel: &mut Self, _, _, cx| {
                                panel.select_tab(i, cx);
                            }))
                            .child(
                                div()
                                    .id(("terminal-tab-close", i))
                                    .flex_none()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(Icon::new(IconName::Close).size_3())
                                    .on_click(cx.listener(move |panel: &mut Self, _, _, cx| {
                                        panel.close_tab(i, cx);
                                    })),
                            )
                    }))
                    .child(div().flex_1())
                    .child(
                        crate::controls::action("add-shell")
                            .ghost()
                            .xsmall()
                            .icon(Icon::new(IconName::Plus))
                            .on_click(cx.listener(|panel: &mut Self, _, window, cx| {
                                panel.open_shell(window, cx);
                            })),
                    ),
            )
            .children(body.map(|view| {
                div()
                    .flex_1()
                    .min_h_0()
                    // The grid draws from its own top-left corner outward, so
                    // without this the first column sits against the panel edge
                    // and the last row against whatever is below it -- a hairline
                    // or the status bar. Matching the tab strip's inset above
                    // puts a shell's first character on the same column as the
                    // tab that names it.
                    //
                    // Costs the shell a column and a row rather than being
                    // painted over them: the view measures its own bounds and
                    // reports the cell count back through the PTY resize, so what
                    // it lays out and what the child believes stay in step.
                    .p_2()
                    .child(view)
                    .into_any_element()
            }))
            .into_any_element()
    }
}

/// Build a terminal palette from the same active theme as the surrounding app.
///
/// ANSI colours remain ANSI colours, but their normal/bright variants come
/// from the theme's adaptive base scales. Default text, background, and cursor
/// use their exact semantic roles, removing the vendored terminal's fixed
/// light-on-charcoal palette from light mode.
fn terminal_palette(cx: &App) -> ColorPalette {
    fn rgb(color: gpui::Hsla) -> (u8, u8, u8) {
        let color = color.to_rgb();
        let channel = |value: f32| (value.clamp(0., 1.) * 255.).round() as u8;
        (channel(color.r), channel(color.g), channel(color.b))
    }

    let theme = cx.theme();
    let background = rgb(theme.background);
    let foreground = rgb(theme.foreground);
    let muted = rgb(theme.muted_foreground);
    let border = rgb(theme.border);
    let ansi_black = if theme.mode.is_dark() {
        background
    } else {
        foreground
    };
    let ansi_white = if theme.mode.is_dark() { muted } else { border };
    let ansi_bright_white = if theme.mode.is_dark() {
        foreground
    } else {
        background
    };
    let cursor = rgb(theme.caret);
    let red = rgb(theme.red);
    let red_bright = rgb(theme.red_light);
    let green = rgb(theme.green);
    let green_bright = rgb(theme.green_light);
    let yellow = rgb(theme.yellow);
    let yellow_bright = rgb(theme.yellow_light);
    let blue = rgb(theme.blue);
    let blue_bright = rgb(theme.blue_light);
    let magenta = rgb(theme.magenta);
    let magenta_bright = rgb(theme.magenta_light);
    let cyan = rgb(theme.cyan);
    let cyan_bright = rgb(theme.cyan_light);

    ColorPalette::builder()
        .background(background.0, background.1, background.2)
        .foreground(foreground.0, foreground.1, foreground.2)
        .cursor(cursor.0, cursor.1, cursor.2)
        .black(ansi_black.0, ansi_black.1, ansi_black.2)
        .red(red.0, red.1, red.2)
        .green(green.0, green.1, green.2)
        .yellow(yellow.0, yellow.1, yellow.2)
        .blue(blue.0, blue.1, blue.2)
        .magenta(magenta.0, magenta.1, magenta.2)
        .cyan(cyan.0, cyan.1, cyan.2)
        .white(ansi_white.0, ansi_white.1, ansi_white.2)
        .bright_black(muted.0, muted.1, muted.2)
        .bright_red(red_bright.0, red_bright.1, red_bright.2)
        .bright_green(green_bright.0, green_bright.1, green_bright.2)
        .bright_yellow(yellow_bright.0, yellow_bright.1, yellow_bright.2)
        .bright_blue(blue_bright.0, blue_bright.1, blue_bright.2)
        .bright_magenta(magenta_bright.0, magenta_bright.1, magenta_bright.2)
        .bright_cyan(cyan_bright.0, cyan_bright.1, cyan_bright.2)
        .bright_white(
            ansi_bright_white.0,
            ansi_bright_white.1,
            ansi_bright_white.2,
        )
        .build()
}

fn hint(text: &'static str, cx: &App) -> impl IntoElement + use<> {
    div()
        .size_full()
        .v_flex()
        .items_center()
        .justify_center()
        .text_color(cx.theme().muted_foreground)
        .child(text)
}
