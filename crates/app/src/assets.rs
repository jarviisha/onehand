//! The window's asset source: onehand's own icons and the Lucide glyphs it
//! picks from the kit's catalog, layered over the ones gpui-component's default
//! bundle (`gpui-kit-assets`) carries.
//!
//! Both halves are needed. The library's components reference `IconName` --
//! and therefore `icons/…` -- in over a hundred places internally (`select`'s
//! chevron, a dialog's close, a toast's dismiss), so dropping the bundled set
//! renders that chrome blank. onehand's own set is served under
//! `onehand/icons/` so the two namespaces cannot collide even where they hold
//! the same Lucide glyph.

use gpui::{AssetSource, Result, SharedString};
use std::borrow::Cow;

pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some(bytes) = crate::icons::embedded(path) {
            return Ok(Some(Cow::Borrowed(bytes)));
        }
        if let Some(bytes) = crate::icons::Picked.load(path)? {
            return Ok(Some(bytes));
        }
        gpui_kit_assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths = gpui_kit_assets::Assets.list(path)?;
        paths.extend(crate::icons::Picked.list(path)?);
        paths.extend(
            crate::icons::all_paths()
                .into_iter()
                .filter(|candidate| candidate.starts_with(path)),
        );
        Ok(paths)
    }
}
