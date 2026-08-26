//! The app's icon registry for the GPUI shell — **brand marks only**.
//!
//! Every UI glyph comes from `gpui_component::IconName`, the enum generated
//! from the 99 SVGs `gpui-component-assets` ships. What is left here is the set
//! that enum cannot hold: a brand mark belongs to the product it stands for,
//! not to a general-purpose UI kit, and no version of that kit is going to
//! start shipping one.
//!
//! The cost of leaning on the library's names is real and worth naming, since
//! it is paid silently: the library **renames icons when it packages them**
//! (its `close.svg` is Lucide's `x`, its `dash` is `minus`, and its `delete` is
//! the backspace key rather than a waste bin), those names carry no guarantee
//! across the pinned revision being bumped, and an icon that fails to resolve
//! draws nothing at all rather than failing to build. Bumping the pin means
//! looking at the app's chrome afterwards.
//!
//! Assets here are served under an `onehand/` prefix so they can never shadow
//! the bundled set at bare `icons/…`, which the library's own components reach
//! for in ~97 places.

use gpui::SharedString;
use gpui_component::IconNamed;

/// Path prefix for onehand's own assets inside the merged [`crate::assets`]
/// source. Must not collide with the bundled library set at `icons/`.
pub const PREFIX: &str = "onehand/icons/";

macro_rules! icons {
    ($($variant:ident => $file:literal),* $(,)?) => {
        /// A compile-time identifier for one checked-in brand mark.
        ///
        /// `dead_code` is allowed for the whole enum on purpose: this is a
        /// *registry*, and its contract is that it mirrors
        /// `assets/icons/manifest.toml` exactly — which is what
        /// `registry_and_manifest_have_the_same_assets` asserts. A variant with
        /// no call site is a curated mark nobody has needed yet, not a mistake;
        /// dropping it to satisfy the lint would break the mirror the test
        /// checks and leave a shipped SVG unreachable.
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        #[allow(dead_code, reason = "a registry mirroring manifest.toml")]
        pub enum Icon {
            $($variant),*
        }

        impl Icon {
            /// Every registered icon, used by the consistency test.
            #[allow(dead_code, reason = "read by the registry consistency test")]
            pub const ALL: &'static [Icon] = &[$(Icon::$variant),*];

            /// The checked-in asset name, shared with the source manifest.
            pub const fn asset_name(self) -> &'static str {
                match self {
                    $(Icon::$variant => $file),*
                }
            }
        }

        /// Resolve an asset path back to its embedded bytes, for [`crate::assets`].
        pub fn embedded(path: &str) -> Option<&'static [u8]> {
            match path.strip_prefix(PREFIX)?.strip_suffix(".svg")? {
                $($file => Some(include_bytes!(
                    concat!("../../../assets/icons/", $file, ".svg")
                ))),*,
                _ => None,
            }
        }

        /// Every path this module serves, for `AssetSource::list`.
        pub fn all_paths() -> Vec<SharedString> {
            vec![$(SharedString::from(concat!("onehand/icons/", $file, ".svg"))),*]
        }
    };
}

impl IconNamed for Icon {
    fn path(self) -> SharedString {
        SharedString::from(format!("{PREFIX}{}.svg", self.asset_name()))
    }
}

icons! {
    ClaudeCode => "claude-code",
}

#[cfg(test)]
mod tests {
    use super::{Icon, embedded};

    /// The registry, the pinned manifest and the checked-in SVGs must agree:
    /// each is easy to update alone, and any two of them agreeing is not enough
    /// to render an icon.
    #[test]
    fn registry_and_manifest_have_the_same_assets() {
        let manifest = include_str!("../../../assets/icons/manifest.toml")
            .parse::<toml::Value>()
            .expect("icon manifest must be valid TOML");
        let mut declared = manifest["icons"]
            .as_table()
            .expect("icon manifest must contain an [icons] table")
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>();
        let mut registered = Icon::ALL
            .iter()
            .map(|icon| icon.asset_name())
            .collect::<Vec<_>>();
        declared.sort_unstable();
        registered.sort_unstable();
        assert_eq!(registered, declared, "registry and manifest differ");
    }

    #[test]
    fn every_icon_resolves_through_the_prefixed_path() {
        for icon in Icon::ALL.iter().copied() {
            let path = format!("onehand/icons/{}.svg", icon.asset_name());
            assert!(
                embedded(&path).is_some_and(|bytes| !bytes.is_empty()),
                "{icon:?} must be embedded at {path}"
            );
        }
        // A bundled library path must fall through to gpui-component-assets.
        assert!(embedded("icons/folder.svg").is_none());
    }
}
