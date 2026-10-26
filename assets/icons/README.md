# Icon assets

`manifest.toml` is the source of truth for every checked-in SVG, and it holds
**only what the bundled set cannot supply**.

Nearly every UI glyph comes from `gpui_component::IconName` instead — the enum
naming the SVGs of `gpui-kit-assets`' default bundle, which the library's own
components already draw from. A glyph that bundle holds no equivalent of is
picked by name from the kit's complete Lucide catalog in
`crates/app/src/icons.rs`, with its reason beside it; nothing is fetched or
checked in for it. Two kinds of thing live here instead:

- a **brand mark**, which belongs to the product it stands for rather than to a
  general-purpose UI kit, and which no version of such a kit is going to start
  shipping;
- a **`-light` fork** of a shape the kit draws, rewritten to a lighter stroke
  for the one place the app draws it oversized; `manifest.toml` names it.

Run the following command after changing a mapping or a pinned version:

```sh
./scripts/sync-icons.sh
```

The script downloads the pinned release into a temporary staging directory and
publishes it only once the complete set is available. Builds never access the
network; every SVG is committed.

> Two earlier sets are gone. The first was Tabler, rewritten from 2px to 2.5px
> on the way in — the thicker outline existed to survive a renderer that
> rasterized small vectors badly. The second was a checked-in Lucide set of 48
> UI glyphs, kept so that app icons did not depend on gpui-component's own
> naming; the app now takes that dependency deliberately. A handful of
> those 48 have since come back, one at a time and each because the bundled
> set turned out to have no drawing of that shape at all; they are now picked
> from the kit's catalog rather than checked in.

## Sources and licenses

- [Lucide](https://github.com/lucide-icons/lucide), ISC — the same upstream the
  bundled set is packaged from, so a fork taken from here matches its
  neighbours' drawing exactly. See `licenses/LUCIDE.txt`.

The bundled UI set now comes from `gpui-kit-assets` (Apache-2.0), which is
a normal cargo dependency rather than a checked-in asset. It is Lucide
underneath, so `licenses/LUCIDE.txt` would have to travel with the binary even
if nothing here were fetched from Lucide directly.

Nothing here currently comes from
[Simple Icons](https://github.com/simple-icons/simple-icons) (CC0-1.0), but
`manifest.toml` still names a brand mark as something that may be checked in and
`sync-icons.sh` still knows how to fetch one. `licenses/SIMPLE_ICONS.md` stays
for that reason and says what has to be reviewed first — brand names and logos
can be protected by trademark whatever the icon set's own licence says.
