# Decisions & references

The choices that reading the code will not explain, and the version pins that have to hold.
`CLAUDE.md` describes what the app *is*; this file explains why it is that way wherever the answer
could otherwise look arbitrary.

Every entry is a constraint **currently in force**, not a history. Change something and edit it here.

---

## 1. Interface

### D1 · The theme is gpui-component's, except the surface ramp

Take the library's radii, typography and whole palette as they are — **except the surfaces**. The app
sets `background` · `muted` · `secondary` · `accent` · `popover` · `border` and the text layers that
sit on them itself, as two overrides written over the library's own config and installed once at boot
(`crate::theme::install`). Everything else — hue, status colours, selection, scrollbar, ring — is
untouched.

**Why deviate at all.** The library's palette is built for panels and forms, where two or three
surfaces are enough. A transcript needs more than that *at once*: a reading ground, a well sunk into
it for machine output, a solid bubble for what the user said, a card floating above for the composer
— and every pair of those appears side by side on screen. The default palette does not have enough
steps. Dark mode was the sharp case: hover, well, bubble and hairline all resolved to one value, so a
quoted command and the user's own message drew identically, and the composer was painted in exactly
the ground it was supposed to float above.

**The boundary is unchanged: still never write a colour, radius or size at a call site** — read it
from `cx.theme()`. Values exist in exactly one place, the ramp in `crate::theme`, and the contrast
each step owes its neighbour is asserted by a test rather than judged by eye. That is why `DESIGN.md`
and `DESIGN-ANSWER.md` describe *structure and behaviour* and not values.

### D2 · Light · dark · follow the system — the user picks

The library ships both palettes, so the app's only job is choosing which one is loaded. The
`appearance` key in `onehand.toml` takes `system` (the default) · `light` · `dark`; it is changed in
the Settings dialog and written straight back to that same file, through an edit-in-place path that
preserves every other entry — choosing a mode must never cost the user their agent list.

`system` **keeps following**, rather than reading once at startup: each window observes the desktop's
appearance, so changing the theme outside the app changes it inside. That is also what settles the
startup race on Linux — the platform is still asking the desktop through the portal and answers with
its default in the meantime, and the real answer arrives later through exactly this observation.

An unrecognized value (a typo) reads as `system` rather than failing the file: the agent list lives in
that same file, and one misspelled word must not take it down.

⚠️ **Precondition:** switching mode re-applies a whole `ThemeConfig`, so the monospace family resolved
for this machine has to be set again after every switch. Miss that and every diff, command and output
block quietly falls back to the body face — precisely the bug the font-resolution step exists to
prevent.

The mode only changes the interface palette. **The embedded terminal has its own ANSI palette** and
does not read the theme, so it looks the same in both modes.

### D3 · The editor is a "quick editor": highlighting, no LSP

Enable the `tree-sitter` feature with **a subset** of the grammars (rust · ts/tsx · js · python · go ·
markdown · toml · yaml · bash · css · html, plus the json that comes with them), not all ~35. Every
surplus grammar is build time and binary size for a language nobody has opened.

⚠️ **Precondition:** turning `tree-sitter` off entirely makes `input_highlighter_factory` return
`Rc::new(|_| None)` — the editor becomes plain text. This feature is the floor, not an option.

### D7 · Plugins are built in and registered once

The first plugin milestone is an in-process composition boundary, not an
extension marketplace. Editor, Files, Neovim and Telegram are separate built-in
plugin crates linked into the Onehand binary. `crates/app/src/plugins.rs`
registers them before the first window and seals the registry; registration is
not available after boot.

`onehand-plugin-api` stays GUI-free and contains stable IDs, descriptors,
capabilities and the API version. Contribution-specific factories and lifecycle
hooks live in `onehand-plugin-host`; there is no general event bus. The Rust API
is version `0.x` and makes no third-party stability promise. External plugins,
when added, use a process protocol rather than dynamic Rust libraries.

This split must not change observable behavior or configuration. In particular,
`onehand.toml`, Workbench labels/order, key bindings, per-root state and
`[remote.telegram]` remain unchanged. Telegram's HTTP/TLS and secret-loading
dependencies belong to its plugin; `onehand-core` owns only the neutral remote
model, access and routing rules.

---

## 2. Version pins

```toml
# gpui-component: pinned by rev, normally.
gpui-component        = { git = "https://github.com/longbridge/gpui-component", rev = "9e3a29d…" }
gpui-component-assets = { git = "https://github.com/longbridge/gpui-component", rev = "9e3a29d…" }

# gpui: NEVER set `rev`.
gpui          = { git = "https://github.com/zed-industries/zed", features = ["profiler"] }
gpui_platform = { git = "https://github.com/zed-industries/zed", features = ["font-kit", "x11", "wayland", "runtime_shaders"] }
```

**Why `gpui` carries no `rev`.** Upstream gpui-component declares `gpui` as a git dependency **with no
rev**, and cargo identifies a git source by **URL + rev**. Adding a rev on our side creates a *second*
source → two different `gpui` crates in the graph → `expected gpui::app::App, found App`. The same
rule applies to `vendor/gpui-terminal`: its `Cargo.toml` must declare `gpui` revless too.

The real pin is **`Cargo.lock`, and the lock file is committed**. Upgrade deliberately:

```bash
cargo update -p gpui --precise <sha>
```

Revisions in use: gpui-component `9e3a29d` · zed `e0931d5`.

---

## 3. Terminal (`vendor/gpui-terminal`)

gpui-component has **no** terminal component. The base is `zortax/gpui-terminal@51f0292`
(MIT/Apache-2.0) — a good render core (PTY + `alacritty_terminal` + grid drawing + box-drawing) with
**no interaction layer at all**: `on_mouse_up` / `on_mouse_move` / `on_scroll` / clipboard were all
empty TODOs, and `mouse.rs` (711 lines) was never imported by `view.rs`. That module is now the wire
format of mouse reporting and nothing else — the selection type, the pixel-to-cell conversion and the
scroll-delta helper it also held were deleted, since the view had grown its own of each against
alacritty's `Selection` rather than the one declared there, so the crate carried two types of the same
name meaning different things.

Everything onehand added is marked `onehand patch`: scrollback, selection, copy/paste
(`Ctrl+Shift+C/V` — Ctrl+C is SIGINT and Ctrl+V is literal-next, and taking either breaks the shell),
bracketed paste, `\r\n` → `\r` normalization, copy-on-select, typing snapping the viewport back to the
bottom, `TERM`/`COLORTERM` set app-side, and a **bounded** PTY read queue with chunk coalescing
(upstream used `flume::unbounded` and notified every 4 KB, so a command writing faster than the parser
grew memory without limit).

Two further patches share one family of bug — *asking for a font that is not there, with nobody
reporting it*:

- **An `EntityInputHandler` for the grid**, so a composing input method (telex, pinyin, kana) works.
  Without one the platform never opens an input context on the terminal, the IME sits in passthrough,
  and raw keys fall straight through to the shell — typing "khoong" produces *khoong*. Preedit is
  drawn over the grid at the cursor (uncommitted text is not the child's yet), and `bounds_for_range`
  is where the candidate window is placed. Its counterpart: `on_key_down` **must** `stop_propagation`
  for every key it encodes itself, because the platform's rule is that an unclaimed key's character
  goes to the input handler — not stopping there types every character twice.
- **Measure the cell from `M`, not from `│`.** Upstream picked a box-drawing character because it
  fills the cell vertically, but shaping falls back *per glyph*: a font without U+2502 returns another
  font's metrics, so the grid is laid out at one font's cell size while the text is drawn in another.
  On top of that the app has to pass the resolved `font_family` down (the vendor's default is the
  string `monospace`, which is a CSS generic and not the name of any family).

**The convention that keeps the vendor readable:** the verbatim import is **one commit** and the
patches are the commit **after it** — so a diff against upstream is exactly what onehand wrote. That
is why `make fmt` / `make lint` are scoped to first-party crates only: a bare `cargo fmt` would
reformat the vendor and destroy that property. The clippy warnings left in the vendor are upstream's;
leave them.

### D4 · Neovim is a Workbench mode, running in a PTY

Cut from the first build because the interaction layer had to be written from scratch. It is written
now, and `Ctrl+Shift+N` opens Neovim on the active project **as the Workbench's third mode**, beside
Editor and Files. No msgpack-RPC embedding: Neovim is a program in a PTY, so what it needed was for
the PTY to be a real terminal, and giving it its own widget would have meant a second renderer, a
second input path and a second set of the same bugs.

**The Workbench and not the terminal dock**, though `onehand-terminal-ui` owns the shared PTY machinery.
The terminal dock is where you run commands and the Workbench is where you work on files; a tab called
`nvim` sitting between two called `zsh` says the editor is a kind of shell. Spawning goes through
`onehand_terminal_ui::spawn_pty` and `Program`, so both grids inherit
one set of rules about `TERM`, the resize callback, the clipboard hook and reaping the child.

The bottom dock is wider, which is the one real argument the other way, and it loses on a detail:
the Workbench keeps its tab group, so `zoomable` returns `PanelControl::Toolbar` and the mode has a
maximize button of its own. The terminal is mounted bare, has no tab bar to put one on, and can only
be enlarged with `Ctrl+Shift+K`, which hides the rail as well.

Three things that mode owes because it is a live PTY rather than an element tree. Its **zoom is a font
size**, not the rem scale the other two bodies are wrapped in — the grid is measured from a shaped
glyph, so scaling its container leaves every column landing past its own character. It takes the key
context **`Terminal`** while showing, which is what gives `Ctrl+S` back to `:w`: that binding is
`Shell && !Terminal` precisely so a program in a PTY keeps it. And **switching to the mode does not
spawn** — the key spawns and then switches, and the empty state carries a *Start Neovim* button, so
the mode strip stays three buttons that change a view rather than two that change a view and one that
launches a process.

What that took, all of it in `vendor/gpui-terminal` and marked `onehand patch`:

- **Answers to the questions a program asks on startup.** Upstream dropped alacritty's
  `Event::PtyWrite` with a comment saying it was handled internally, which is the opposite of what it
  means: that event *is* the answer to Device Attributes, the cursor position report and the version
  query, handed out because alacritty has no idea where the PTY is. `Event::ColorRequest` went the
  same way, and that one is how a program reads the background colour to decide whether it is drawing
  on light or dark. Both are now routed back, the colour resolved against the palette in force rather
  than one copied at construction — so the editor's colour scheme follows the app's appearance.
- **Mouse reporting**, with the encoding chosen by what the child asked for. SGR (1006) where it
  enabled it, the legacy byte form otherwise — not a detail: sending SGR to a program that only asked
  for 1000 delivers an escape sequence it cannot parse, so a click *types* `[<0;40;12M` into it.
  Motion is reported per cell rather than per pixel, and all three buttons are forwarded.
- **The `Shift` bypass**, which is what keeps the terminal usable underneath. A program tracking the
  mouse takes click, drag and wheel completely, leaving no way to select a line of its output and copy
  it — and wanting to do that is most of why anyone is looking at the output. Holding shift takes the
  gesture back. It costs the child nothing: no terminal delivers shift+click, so nothing is written to
  want it.
- **Wheel on the alternate screen becomes arrow keys** when the program is not tracking the mouse.
  There is no scrollback there, so a wheel the terminal kept for itself did nothing at all.
- **`DECSCUSR`.** Already parsed by alacritty and thrown away by the renderer, which drew a filled
  block always — and drew it *after* the glyph pass, so the cursor did not sit on a character, it hid
  one. Shape, `DECTCEM` hiding, a hollow outline when the grid does not hold focus, and the character
  repainted over the block. Blinking is deliberately left out: it needs a repaint on a timer for the
  life of every tab, in a view that otherwise draws only when bytes arrive.
- **`OSC 52`, write only.** A yank to the system clipboard is the only way a copy inside a full-screen
  editor reaches anything outside the terminal. The **read** half is refused on purpose: answering it
  hands whatever the user last copied to whatever is running in the terminal, including something at
  the far end of an ssh session, which is why xterm ships it disabled.
- **Focus reporting** (mode 1004). An editor asks for this so it can re-read a file written while the
  user was elsewhere — and in an app whose point is an agent editing those same files, "elsewhere" is
  one click away and constant. Without it Neovim shows a copy of a file that no longer exists and has
  no reason to suspect it. The window's activation counts as well as the focus tree's: the caret being
  in the grid while the window sits behind another application is not having the keyboard.
- **The attributes that colour a cell**, through one function used by the background pass, the glyph
  pass and the cursor alike. `INVERSE` is how most colour schemes draw a status line, a visual
  selection and a search hit; unswapped they came out dark on dark, which reads as a broken theme
  rather than a missing attribute. `DIM` and `HIDDEN` come with it — the second matters off the screen
  as well as on it, since hiding typed input is what it exists for. **One function and three callers
  is the substance here**: three places deriving the same colours separately is exactly how the cursor
  came to be painted over the character underneath it.
- **Every underline the protocol has, in the colour the program picked.** Only plain `UNDERLINE` was
  read, and `UNDERCURL` is a *different bit* — so a language server's diagnostics drew no underline at
  all. The colour comes from `Cell::underline_color`, which is the half that carries the meaning: an
  error and a warning are the same squiggle in different colours. Curly is exact; double, dotted and
  dashed fall back to straight, because GPUI's underline is a thickness, a colour and a wavy flag.
  Strikethrough was hard-coded to `None` and is now drawn.
- **Modified keys.** Every cursor, navigation and function key can be pressed with Shift, Alt or
  Control, and the plain sequence says nothing about that — so an editor told to move by word on
  `Ctrl+Right` received a plain `Right` and moved by one character, a binding that appears configured
  and quietly does the wrong thing. Also `Ctrl+Alt+key`, which used to arrive as the plain `Ctrl`
  press, and Alt on a non-ASCII layout, which used to be dropped entirely.

IME inside the PTY was on this list and landed first (see section 3): it could not wait for Neovim,
because a terminal you cannot type Vietnamese into is broken at the command line already.

**Still not done: `APP_KEYPAD`.** The numeric keypad's application mode is unimplemented, because gpui
does not distinguish a keypad key from the digit above it — the fix is upstream of this crate, and the
keys still work, they simply always send the ordinary form.

---

## 4. Icons

### D6 · UI icons come from `IconName`; `assets/icons/` holds only what it cannot draw

**The bundled set has to ship.** There are ~97 places inside gpui-component that call `IconName::…`
themselves (`select` → `ChevronDown`; `dock/tab_panel` → `Ellipsis` / `Maximize` / `PanelLeft`; dialog
→ `Close`…). Not providing it through `AssetSource` makes select's chevron and the dock tab's buttons
**render blank**. onehand now draws its own chrome from that same set.

| Source | Content | Used by |
|---|---|---|
| `gpui-component-assets` (99 SVG, Apache-2.0) | nearly every UI glyph | the library **and** onehand's code, through `IconName` |
| `assets/icons/` (2 SVG: CC0-1.0 + ISC) | what that enum cannot hold | onehand's code, through `impl IconNamed` |

The boundary is **what the bundled set cannot draw**, not "app versus library". Two things fall
outside it. A *brand mark*: a product's mark belongs to that product, and no version of a
general-purpose UI kit is going to start shipping one. And a *missing shape*: a glyph the bundled set
holds no equivalent of at all — added one at a time, fetched from Lucide, which is the same upstream
gpui-component packages, so the stroke weight matches its neighbours exactly.

That second half is a **narrow amendment**, and the thing it must not become is a slow return of the
48-glyph set this decision deleted. The test is the drawing, not the name: `IconName::Delete` being
the ⌫ key rather than a waste bin is an *approximation*, and approximations stay. Only an absence
qualifies. So far exactly one has: the transcript's *Changed* activity group, whose subject is a file
edited in place, and for which the bundled set offers no pencil of any kind — `Replace` is a
find-and-replace mark, two boxes swapped, which is what that group is not. It is now
`assets/icons/square-pen.svg`, and the reason rides next to the manifest entry rather than here.

#### The cost, written down because it is paid silently

This **reverses** the earlier decision — the 48 self-hosted Lucide SVGs were deleted. The old reasons
are all still true word for word; they are simply no longer the ones being chosen:

- gpui-component **renames icons when it packages them**. Its `close.svg` is really `lucide-x`, `dash`
  is `lucide-minus`, and `delete` is the backspace key rather than a waste bin. ⚠️ **Comparing by name
  is not trustworthy** — compare by the `class="lucide-*"` attribute inside the SVG.
- The names in `IconName` are *their* surface, with no guarantee across versions, while this repo pins
  a `rev` and will bump it over time.
- An icon that fails to resolve **does not break the build**, it draws a blank. Bumping the
  gpui-component rev means looking at the app's chrome afterwards — no test catches this.

#### 24 places using an approximate glyph

The bundled set covers only 21 of the 49 former icons. The remaining 24 are approximations rather than
equivalents — notably `attach` → `Inbox`, `@` → `Asterisk`, `/` → `Dash`, `pin` → `Star`,
`trash` → `Delete` (the ⌫ key), `stop` → `Pause`, `clock` → `Calendar`, `tool` → `Settings2`.
`file-text` / `file-diff` / `file-code` all collapse into `File`, so a tool card no longer
distinguishes read from edit from run by shape. One of the 24 has since been withdrawn rather than
approximated — `edit` → `Replace`, above.

`assets/icons/manifest.toml` is the source of truth for what is checked in; `scripts/sync-icons.sh`
knows two providers and no more. To add one: edit the manifest, with the reason beside the entry →
run the script → register it in the `icons!` macro. A test keeps the three in agreement.

**Licensing:** `assets/icons/licenses/` — `claude-code` is Simple Icons, CC0-1.0; `square-pen` is
Lucide, ISC; the bundled `gpui-component-assets` is Apache-2.0 and is Lucide underneath, so that ISC
notice has to travel with the binary either way. All permissive, no conflict.

---

## 5. Not built

Listed because a missing feature nobody wrote down reads as a bug in the ones that exist.

- **Command palette** (`Ctrl+Shift+P`) — a *feature* (a command registry plus a filtered popup), not a
  line in the keymap.
- **Keyboard navigation for the completion popup.** gpui-component's completion menu lives *inside*
  the input, so it answers `up`/`down` before the caret can move; the hook that would reach it
  (`CompletionProvider`) is documented as editor-only — *"an ordinary input or textarea has no
  language server, and no field to reach one through"*. GPUI dispatches actions from leaf to root, so
  a binding placed on the composer's wrapper cannot intercept the focused input either. Today the
  popup is click-to-pick, with Enter accepting the highlighted row. Two ways out: ask upstream for a
  hook, or move the composer to `EditorState` (more expensive — that is a 17.5k-line code editor).
- **`path:line:col` in agent prose is not clickable.** The transcript renders prose through
  `TextView::markdown` and does not scan it for path tokens. Only a tool card's path header opens a
  file, and it carries no line — ACP's diff payload has no hunk offsets.
  `onehand_core::parse::parse_path_line` is the parser that feature needs and **has no caller today**.
- **Folding a code block inside prose.** `TextViewStyle::code_block` is *one* style for every block,
  so per-block fold state has nowhere to live. Reaching it would mean replacing gpui-component's code
  block renderer with a custom block parser — trading syntax highlighting for a chevron. Instead: a
  height cap plus a Copy button on each block.
- **`[font]` and `[icons]` in the config** still parse but are ignored (D1 replaced them with
  gpui-component's theme). They are kept so existing config files do not break, and they are the
  obvious hook if per-role icon tinting comes back.

---

## 6. Invariants

- `cargo tree -p onehand-core -i gpui` must **fail with "did not match any packages"**. That is why
  core exists. (Use `-i`, not `| grep gpui` — the checkout directory can contain that word in its
  path.)
- **Core dictates no runtime.** Blocking functions plus thin async wrappers. GPUI runs on smol and has
  no tokio reactor, so a core awaiting tokio I/O directly would panic inside the UI process.
- Shared logic lives in core and is not restated in the front end.
- Every UI glyph goes through `gpui_component::IconName`; `crate::icons::Icon` holds only what that
  enum cannot draw at all (§4).
- `impl IntoElement` returned from a view-building function: consider `+ use<>` — edition 2024
  captures every lifetime.
- Only `assets` and `shell` are `pub` in `crates/app`; every other module is private, so rustc's
  `dead_code` analysis can still see through them (see `crates/app/src/lib.rs`).
- **Documentation is written in English.** See the rule in `CLAUDE.md`; a test enforces it.

---

## 7. External sources

| What | Where |
|---|---|
| gpui-component | <https://github.com/longbridge/gpui-component> — Apache-2.0 |
| Docs / gallery | <https://longbridge.github.io/gpui-component> · `cargo run` in that repo opens the story gallery |
| gpui (Zed) | <https://github.com/zed-industries/zed> — `crates/gpui` |
| gpui-terminal (vendored) | <https://github.com/zortax/gpui-terminal> @ `51f0292` — MIT/Apache-2.0 |
| Lucide | <https://lucide.dev> — ISC. Reaches us only inside `gpui-component-assets`; nothing here fetches it any more |
| Simple Icons | <https://github.com/simple-icons/simple-icons> — CC0-1.0, fetched by `scripts/sync-icons.sh` |
| ACP | <https://agentclientprotocol.com> |

**Read the source, not the docs.** Twice now, grepping for a concept's name in an unfamiliar repo gave
the wrong answer — grep is only enough to *rule out*; confirming means reading the call site. Clone
gpui-component into `/tmp` and read the real files: `crates/ui/src/dock/` · `crates/ui/src/sidebar/` ·
`crates/ui/src/text/text_view.rs` · `crates/base/src/input/` · `crates/ui/src/theme/` ·
`crates/ui/src/list/` · `crates/story/src/stories/` (real usage examples for every component).
