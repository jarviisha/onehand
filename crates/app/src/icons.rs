//! The app's icon registry for the GPUI shell — **what the bundled set cannot
//! supply, and nothing else**.
//!
//! Nearly every UI glyph comes from `gpui_component::IconName`, the enum
//! generated from the 99 SVGs `gpui-component-assets` ships. Two kinds of thing
//! that enum cannot hold are checked in here instead. A **brand mark** belongs
//! to the product it stands for rather than to a general-purpose UI kit, and no
//! version of that kit is going to start shipping one. A **missing shape** is a
//! glyph the bundled set holds no equivalent of at all — added one at a time,
//! with the reason recorded beside the manifest entry, and never merely because
//! a name there is unlovely. The repo has already carried a self-hosted set of
//! 48 UI glyphs and deleted it; the point of that deletion was one stroke
//! weight across the library's chrome and the app's, and an entry here that
//! duplicates a shape the library already draws spends exactly that.
//!
//! **Which is what the `-light` entries spend, deliberately and once.** The
//! stroke weight lives inside the file and no API reaches it, so a glyph drawn
//! much larger than the app draws glyphs anywhere else cannot be made lighter
//! without a second copy of it. The composer's action row is that place: it
//! letters its icons at 1.25rem where the rest of the app draws them at about
//! 0.75rem, and the upstream stroke of 2 reads as a marker pen at that size.
//! What keeps the cost to one place is that the library's own copies stay in
//! use at every other call site -- so the app carries two weights split by *how
//! big a glyph is drawn*, which is a rule, rather than by which glyph it is,
//! which would be taste. A third weight, or a `-light` entry whose call site is
//! not drawn oversized, is the door reopening.
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
        /// A compile-time identifier for one checked-in SVG.
        ///
        /// `dead_code` is allowed for the whole enum on purpose: this is a
        /// *registry*, and its contract is that it mirrors
        /// `assets/icons/manifest.toml` exactly — which is what
        /// `registry_and_manifest_have_the_same_assets` asserts. A variant with
        /// no call site is a curated asset nobody has needed yet, not a mistake;
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
    ArrowUpLight => "arrow-up-light",
    AtSign => "at-sign",
    GitBranch => "git-branch",
    LogOut => "log-out",
    Paperclip => "paperclip",
    PlusLight => "plus-light",
    Shield => "shield",
    SquarePen => "square-pen",
    SquareSlash => "square-slash",
    Trash => "trash-2",
    Zap => "zap",
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

    /// The stroke overrides are applied by the sync script, into files this
    /// repo then ships. Nothing at build time reruns that script, so the shipped
    /// bytes are the only evidence it ran -- and the one failure mode that
    /// leaves no trace is a sync performed before the override existed, or a
    /// weight edited back by hand. Read the files and check.
    #[test]
    fn every_stroke_override_names_a_fetched_icon_and_reached_its_file() {
        let manifest = include_str!("../../../assets/icons/manifest.toml")
            .parse::<toml::Value>()
            .expect("icon manifest must be valid TOML");
        let fetched = manifest["icons"]
            .as_table()
            .expect("icon manifest must contain an [icons] table");
        let Some(strokes) = manifest.get("stroke").and_then(toml::Value::as_table) else {
            return;
        };
        for (name, width) in strokes {
            assert!(
                fetched.contains_key(name),
                "{name} has a stroke override but is not fetched"
            );
            let width = width
                .as_str()
                .expect("a stroke width is written as a string");
            let icon = Icon::ALL
                .iter()
                .copied()
                .find(|icon| icon.asset_name() == name)
                .expect("registry and manifest agree, checked above");
            let svg = embedded(&format!("onehand/icons/{name}.svg")).expect("embedded");
            let svg = std::str::from_utf8(svg).expect("an SVG is text");
            assert!(
                svg.contains(&format!("stroke-width=\"{width}\"")),
                "{icon:?} ships at a different weight than the manifest declares"
            );
        }
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
