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
/// Lucide `square-pen`: Rename in a session's menu.
pub const SQUARE_PEN: &str = "labs/icons/square-pen.svg";
/// Lucide `circle-dot`: the Issues page in the rail.
pub const CIRCLE_DOT: &str = "labs/icons/circle-dot.svg";

/// Lucide `hand`: a session that needs input.
pub const HAND: &str = "labs/icons/hand.svg";
/// Lucide `circle`: an idle session.
pub const CIRCLE: &str = "labs/icons/circle.svg";
/// Lucide `list-filter`: the rail's session filter.
pub const LIST_FILTER: &str = "labs/icons/list-filter.svg";

/// Lucide `square`: Stop on a running session.
pub const SQUARE: &str = "labs/icons/square.svg";
/// Lucide `archive`: Archive on a session.
pub const ARCHIVE: &str = "labs/icons/archive.svg";

const EMBEDDED: [(&str, &[u8]); 10] = [
    (ZAP, include_bytes!("../assets/icons/zap.svg")),
    (GIT_BRANCH, include_bytes!("../assets/icons/git-branch.svg")),
    (SHIELD, include_bytes!("../assets/icons/shield.svg")),
    (SQUARE_PEN, include_bytes!("../assets/icons/square-pen.svg")),
    (CIRCLE_DOT, include_bytes!("../assets/icons/circle-dot.svg")),
    (HAND, include_bytes!("../assets/icons/hand.svg")),
    (CIRCLE, include_bytes!("../assets/icons/circle.svg")),
    (
        LIST_FILTER,
        include_bytes!("../assets/icons/list-filter.svg"),
    ),
    (SQUARE, include_bytes!("../assets/icons/square.svg")),
    (ARCHIVE, include_bytes!("../assets/icons/archive.svg")),
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
