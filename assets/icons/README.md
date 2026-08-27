# Icon assets

`manifest.toml` is the source of truth for every checked-in SVG, and it holds
**only what the bundled set cannot supply**.

Nearly every UI glyph comes from `gpui_component::IconName` instead — the enum
generated from the SVGs `gpui-component-assets` ships, which the library's own
components already draw from. Two kinds of thing live here instead:

- a **brand mark**, which belongs to the product it stands for rather than to a
  general-purpose UI kit, and which no version of such a kit is going to start
  shipping;
- a **missing shape**: a glyph the bundled set holds no equivalent of at all,
  added one at a time with its reason written beside the manifest entry. A name
  in `IconName` that merely reads oddly is not one of these — if the library
  already draws the shape, the app draws it from there.

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
> naming; the app now takes that dependency deliberately, and the trade is
> written up in the icon decision. Exactly one of those 48 has since come back,
> because the bundled set turned out to have no drawing of it at all.

## Sources and licenses

- [Simple Icons](https://github.com/simple-icons/simple-icons), CC0-1.0. Brand
  names and logos may still be protected by trademark; see
  `licenses/SIMPLE_ICONS.md`.
- [Lucide](https://github.com/lucide-icons/lucide), ISC — the same upstream the
  bundled set is packaged from, so a missing shape taken from here matches its
  neighbours' stroke weight exactly. See `licenses/LUCIDE.txt`.

The bundled UI set now comes from `gpui-component-assets` (Apache-2.0), which is
a normal cargo dependency rather than a checked-in asset. It is Lucide
underneath, so `licenses/LUCIDE.txt` would have to travel with the binary even
if nothing here were fetched from Lucide directly.
