//! Shared PTY ownership and terminal-grid setup used by the shell dock and
//! built-in Neovim plugin. Rendering and input remain in the main GPUI process.

use gpui::{App, AppContext as _, Entity, SharedString, Window};
use gpui_component::ActiveTheme as _;
use gpui_terminal::{ColorPalette, TerminalConfig, TerminalView};
use portable_pty::{CommandBuilder, NativePtySystem, PtySize, PtySystem as _};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Scrollback kept per terminal: history is bytes in RAM per tab, and nobody
/// scrolls back further than this without reaching for the shell's own pager.
const SCROLLBACK: usize = 2000;

/// What a PTY was started to run.
///
/// The two live in one enum because everything around them is shared — the
/// `TERM` the child inherits, the resize callback, the clipboard hook and the
/// reaping below are the same question whichever program it is, and answering
/// them twice is how the two answers start to differ.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Program {
    /// The user's login shell.
    Shell,
    /// Neovim, opened on a project root.
    Neovim,
}

/// One live PTY: its grid, the child, and the handle that resizes it.
///
/// The fields are private on purpose: what an owner needs is [`Self::view`] to
/// render and the value itself to keep alive, and a `child` reachable from
/// outside is one somebody can kill without the grid ever learning about it.
pub struct PtyTab {
    view: Entity<TerminalView>,
    label: SharedString,
    /// Kept alive for the tab's life. Dropping the PTY pair hangs up the
    /// terminal, which is what gets a well-behaved shell to leave; [`Drop`]
    /// below is what handles the rest.
    ///
    /// Shared with the view's resize callback, which is why it is behind an
    /// `Arc<Mutex<_>>` rather than owned outright — see [`spawn_pty`]. The
    /// callback is `Send + Sync`, and a `Mutex` is what makes a `MasterPty`
    /// that is only `Send` usable from one.
    _pty: Arc<Mutex<Box<dyn portable_pty::MasterPty + Send>>>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
}

impl PtyTab {
    /// The grid, to mount.
    pub fn view(&self) -> &Entity<TerminalView> {
        &self.view
    }
    pub fn label(&self) -> SharedString {
        self.label.clone()
    }

    /// Whether the child has exited.
    ///
    /// Asked rather than remembered, and asked of the process rather than of
    /// the event that announced it: `try_wait` is the only thing that actually
    /// knows, so a tab holding a dead child is caught however it died — `:q`,
    /// `exit`, a crash, or a `kill` from somewhere else entirely.
    ///
    /// Also reaps it, which is the second half of why this is worth calling: a
    /// child that exited but was never waited on stays a zombie until the whole
    /// app ends.
    pub fn finished(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(Some(_)))
    }

    /// Re-apply a font size to this grid.
    ///
    /// The grid is a *measured* glyph lattice, so its zoom is a font size rather
    /// than the rem scale every other panel uses: changing it re-measures the
    /// cell, which changes the cell count, which resizes the PTY. Wrapping a
    /// grid in a rem scale instead would stretch the box around it and leave
    /// every column landing past its glyph.
    pub fn set_font_size(&self, size: gpui::Pixels, cx: &mut App) {
        self.view.update(cx, |view, cx| {
            view.update_config(
                TerminalConfig {
                    font_size: size,
                    ..view.config().clone()
                },
                cx,
            );
        });
    }

    /// Re-apply a palette to this grid, after an appearance change.
    pub fn set_palette(&self, colors: ColorPalette, cx: &mut App) {
        self.view.update(cx, |view, cx| {
            view.update_config(
                TerminalConfig {
                    colors,
                    ..view.config().clone()
                },
                cx,
            );
        });
    }
}

/// Close the tab's child for real.
///
/// Dropping the PTY alone is not enough twice over: a child that ignores
/// `SIGHUP` (`nohup`, anything that called `setsid`) keeps running with no tab
/// left to show it, and a child that *did* exit stays a zombie until the process
/// ends, because nobody reaps it.
///
/// `kill` then `wait`, in that order and unconditionally — `kill` on an
/// already-dead child is a no-op, and `wait` is what actually collects it.
impl Drop for PtyTab {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Theme inputs that affect the terminal palette.
///
/// Kept separately because `ColorPalette` intentionally hides its ANSI table.
/// Comparing these lets a render update live terminals once per appearance
/// change instead of rewriting their config on every frame.
#[derive(Clone, Copy, PartialEq)]
pub struct TerminalThemeKey {
    dark: bool,
    colors: [gpui::Hsla; 17],
}

impl TerminalThemeKey {
    pub fn current(cx: &App) -> Self {
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

/// The program a tab runs, and the name its tab carries.
///
/// Neovim is looked up on `PATH` here rather than handed to the PTY to fail on,
/// because the two failures do not read alike: a spawn that fails comes back as
/// "No such file or directory" with no subject, and the one thing the user needs
/// told is *which* file. It is a plain `nvim` and not `$EDITOR` — this is the
/// Neovim key, and honouring `$EDITOR` would make it open `vi` or `nano` for
/// someone who set that variable years ago for `git commit`.
fn command(program: Program) -> Result<(CommandBuilder, SharedString), String> {
    match program {
        Program::Shell => {
            let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
            let label = Path::new(&shell)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| shell.clone());
            Ok((CommandBuilder::new(&shell), label.into()))
        }
        Program::Neovim => {
            let found = std::env::var_os("PATH")
                .into_iter()
                .flat_map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
                .map(|dir| dir.join("nvim"))
                .find(|candidate| candidate.is_file())
                .ok_or_else(|| "Neovim is not installed, or `nvim` is not on PATH".to_string())?;
            Ok((CommandBuilder::new(found), "nvim".into()))
        }
    }
}

/// Start `program` in `cwd`.
///
/// Shared because the Workbench spawns Neovim through it and the terminal dock
/// spawns a login shell: every rule below — the inherited `TERM`, the resize
/// callback the vendored view cannot install for itself, the clipboard hook, the
/// child that has to be reaped — belongs to *running a program in a PTY* rather
/// than to either panel, and a second copy of them is a second copy to keep in
/// step.
///
/// The font family is taken from the resolved theme rather than assumed. A
/// family name is only a request and a missing one fails silently, and this is
/// the sharpest case there is: the grid is *measured* from a shaped glyph, so a
/// family that does not resolve does not merely change the typeface — the cell
/// is sized from one font while the row is drawn in another and every column
/// lands past its glyph.
///
/// `on_exit` is how the owner learns the child is gone. **It has to be told**:
/// nothing else notices. The grid keeps rendering the last screen the child
/// drew, which after `:q` or `exit` is an empty one with a cursor on it, and the
/// tab sits there taking keystrokes nothing will ever read.
pub fn spawn_pty(
    cwd: &PathBuf,
    program: Program,
    font_size: gpui::Pixels,
    cx: &mut App,
    on_exit: impl Fn(&mut Window, &mut App) + 'static,
) -> Result<PtyTab, String> {
    let config = TerminalConfig {
        scrollback: SCROLLBACK,
        font_size,
        colors: terminal_palette(cx),
        font_family: cx.theme().mono_font_family.to_string(),
        ..TerminalConfig::default()
    };
    // Opened at the config's own grid size so the PTY and the client-side grid
    // start out agreeing. Neither number survives the first paint -- that is
    // where the real geometry is measured and pushed through the resize callback
    // below -- but starting them apart means the shell prints its prompt at one
    // width and is told another a frame later.
    let pty = NativePtySystem::default()
        .openpty(PtySize {
            rows: config.rows as u16,
            cols: config.cols as u16,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| e.to_string())?;
    let (mut command, label) = command(program)?;
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
    let master = Arc::new(Mutex::new(pty.master));
    let view = cx.new(|cx| {
        TerminalView::new(writer, reader, config, cx)
            // The one thing the vendored view cannot do for itself.
            //
            // It re-measures the grid on every paint and resizes its own `Term`
            // when the cell count changes -- but the *PTY* only learns about it
            // through this callback, and upstream leaves it unset. Without it
            // the child's winsize stays at whatever `openpty` was given for the
            // life of the tab: `$COLUMNS`/`$LINES` never move, and every
            // full-screen program (`less`, `htop`, `vim`, `git log`'s pager)
            // draws an 80x24 box inside a panel of some other size. The
            // client-side grid resizing correctly is what makes it look like a
            // rendering bug rather than a missing ioctl.
            //
            // This also covers zoom for free: a font-size change re-measures the
            // cell, which changes the cell count, which comes back through here.
            .with_resize_callback({
                let master = master.clone();
                move |cols, rows| {
                    if let Ok(master) = master.lock() {
                        // A failed resize is not worth tearing the tab down for:
                        // the grid has already resized, so the panel stays
                        // usable and only the child's idea of its size is stale.
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
            // The child is gone; tell whoever is holding this tab.
            //
            // **Deferred, and it has to be.** This fires from inside the grid's
            // own render, and the panel that owns the tab is the thing currently
            // rendering that grid -- so it is already being updated, and
            // reaching into it here is a panic rather than an error. Which is
            // the worst possible shape for this particular bug: the app would go
            // down at the moment somebody typed `:q`.
            //
            // `Window::defer` rather than the app's, because putting the caret
            // somewhere it can still be reached needs the window, and a grid
            // that has just been dropped while holding focus takes the whole
            // keymap with it.
            .with_exit_callback({
                let on_exit = std::rc::Rc::new(on_exit);
                move |window, cx| {
                    let on_exit = on_exit.clone();
                    window.defer(cx, move |window, cx| on_exit(window, cx));
                }
            })
    });
    Ok(PtyTab {
        view,
        label,
        _pty: master,
        child,
    })
}

/// Build a terminal palette from the same active theme as the surrounding app.
///
/// ANSI colours remain ANSI colours, but their normal/bright variants come from
/// the theme's adaptive base scales. Default text, background and cursor use
/// their exact semantic roles, which is what removes the vendored terminal's
/// fixed light-on-charcoal palette from light mode.
pub fn terminal_palette(cx: &App) -> ColorPalette {
    fn rgb(color: gpui::Hsla) -> (u8, u8, u8) {
        let color = color.to_rgb();
        let c = |v: f32| (v.clamp(0., 1.) * 255.).round() as u8;
        (c(color.r), c(color.g), c(color.b))
    }
    let t = cx.theme();
    let background = rgb(t.background);
    let foreground = rgb(t.foreground);
    let muted = rgb(t.muted_foreground);
    let border = rgb(t.border);
    let black = if t.mode.is_dark() {
        background
    } else {
        foreground
    };
    let white = if t.mode.is_dark() { muted } else { border };
    let bright_white = if t.mode.is_dark() {
        foreground
    } else {
        background
    };
    let red = rgb(t.red);
    let red_b = rgb(t.red_light);
    let green = rgb(t.green);
    let green_b = rgb(t.green_light);
    let yellow = rgb(t.yellow);
    let yellow_b = rgb(t.yellow_light);
    let blue = rgb(t.blue);
    let blue_b = rgb(t.blue_light);
    let magenta = rgb(t.magenta);
    let magenta_b = rgb(t.magenta_light);
    let cyan = rgb(t.cyan);
    let cyan_b = rgb(t.cyan_light);
    let cursor = rgb(t.caret);
    ColorPalette::builder()
        .background(background.0, background.1, background.2)
        .foreground(foreground.0, foreground.1, foreground.2)
        .cursor(cursor.0, cursor.1, cursor.2)
        .black(black.0, black.1, black.2)
        .red(red.0, red.1, red.2)
        .green(green.0, green.1, green.2)
        .yellow(yellow.0, yellow.1, yellow.2)
        .blue(blue.0, blue.1, blue.2)
        .magenta(magenta.0, magenta.1, magenta.2)
        .cyan(cyan.0, cyan.1, cyan.2)
        .white(white.0, white.1, white.2)
        .bright_black(muted.0, muted.1, muted.2)
        .bright_red(red_b.0, red_b.1, red_b.2)
        .bright_green(green_b.0, green_b.1, green_b.2)
        .bright_yellow(yellow_b.0, yellow_b.1, yellow_b.2)
        .bright_blue(blue_b.0, blue_b.1, blue_b.2)
        .bright_magenta(magenta_b.0, magenta_b.1, magenta_b.2)
        .bright_cyan(cyan_b.0, cyan_b.1, cyan_b.2)
        .bright_white(bright_white.0, bright_white.1, bright_white.2)
        .build()
}
