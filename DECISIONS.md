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
empty TODOs, and `mouse.rs` (711 lines) was never imported by `view.rs`.

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
is why `make fmt` / `make lint` are scoped to `-p onehand -p onehand-core`: a bare `cargo fmt` would
reformat the vendor and destroy that property. The clippy warnings left in the vendor are upstream's;
leave them.

### D4 · No Neovim mode yet

Cut from the first build because the interaction layer had to be written from scratch. This is the
harshest parity work — mouse reporting 64/65, Shift bypass, OSC 52, DECSCUSR cursor shapes. IME inside
the PTY was on this list and is now **done** (see section 3): it could not wait for Neovim, because a
terminal you cannot type Vietnamese into is broken at the command line already. The rest is estimated
at ~700–900 lines. `Ctrl+Shift+N` is not bound.

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

- **Neovim mode** (`Ctrl+Shift+N`) — see D4.
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
