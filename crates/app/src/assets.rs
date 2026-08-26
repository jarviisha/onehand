//! The window's asset source: onehand's own icons layered over the ones
//! gpui-component bundles.
//!
//! Both halves are needed. gpui-component's components reference `IconName` --
//! and therefore `icons/…` -- in ~97 places internally (`select`'s chevron,
//! `dock/tab_panel`'s ellipsis and panel toggles, dialog's close), so dropping
//! the bundled set renders that chrome blank. onehand's own set is served under
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
        gpui_component_assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths = gpui_component_assets::Assets.list(path)?;
        paths.extend(
            crate::icons::all_paths()
                .into_iter()
                .filter(|candidate| candidate.starts_with(path)),
        );
        Ok(paths)
    }
}
