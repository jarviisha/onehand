//! gpui-component's icons, plus the few shapes its set has no drawing of,
//! embedded from `assets/icons/` (Lucide, copied from the app's synced set).
use gpui::{AssetSource, Result, SharedString};
use std::borrow::Cow;

/// Lucide `zap`: the Fast chip.
pub const ZAP: &str = "labs/icons/zap.svg";

/// Lucide `git-branch`: the branch under the composer.
pub const GIT_BRANCH: &str = "labs/icons/git-branch.svg";
/// Lucide `shield`: the permission mode in the composer's toolbar.
pub const SHIELD: &str = "labs/icons/shield.svg";

const EMBEDDED: [(&str, &[u8]); 3] = [
    (ZAP, include_bytes!("../assets/icons/zap.svg")),
    (GIT_BRANCH, include_bytes!("../assets/icons/git-branch.svg")),
    (SHIELD, include_bytes!("../assets/icons/shield.svg")),
];

pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        match EMBEDDED.iter().find(|(p, _)| *p == path) {
            Some((_, bytes)) => Ok(Some(Cow::Borrowed(bytes))),
            None => gpui_component_assets::Assets.load(path),
        }
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths = gpui_component_assets::Assets.list(path)?;
        paths.extend(
            EMBEDDED
                .iter()
                .filter(|(p, _)| p.starts_with(path))
                .map(|(p, _)| SharedString::from(*p)),
        );
        Ok(paths)
    }
}
