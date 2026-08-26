# Icon assets

`manifest.toml` is the source of truth for every checked-in SVG, and it holds
**brand marks only**.

Every UI glyph comes from `gpui_component::IconName` instead — the enum
generated from the SVGs `gpui-component-assets` ships, which the library's own
components already draw from. A brand mark is what that set cannot supply: it
belongs to the product it stands for rather than to a general-purpose UI kit.

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
> written up in the icon decision.

## Sources and licenses

- [Simple Icons](https://github.com/simple-icons/simple-icons), CC0-1.0. Brand
  names and logos may still be protected by trademark; see
  `licenses/SIMPLE_ICONS.md`.

The bundled UI set now comes from `gpui-component-assets` (Apache-2.0), which is
a normal cargo dependency rather than a checked-in asset. `licenses/LUCIDE.txt`
is kept because that bundled set is Lucide underneath and its ISC notice still
has to travel with the binary.
