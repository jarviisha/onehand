//! The app's icon registry for the GPUI shell — **what the bundled set does not
//! serve, and nothing else**.
//!
//! Nearly every UI glyph comes from `gpui_component::IconName`, the enum
//! naming the 106 SVGs of `gpui-kit-assets`' default bundle, which the window
//! serves whole. Three kinds of thing that enum cannot hold are listed here:
//!
//! - A **missing shape**: a glyph the default bundle holds no equivalent of at
//!   all, picked from the kit's complete Lucide catalog by name. The kit embeds
//!   only the ones picked, so each is added one at a time with its reason
//!   beside it, and never merely because a name in `IconName` is unlovely. Its
//!   drawing is the kit's, at the kit's stroke weight, so it stands beside the
//!   library's own glyphs as one of them.
//! - A **brand mark**, which belongs to the product it stands for rather than
//!   to a general-purpose UI kit. Checked in through `assets/icons/manifest.toml`.
//! - A `-light` **fork**, deliberately and once. The stroke weight lives inside
//!   the file and no API reaches it, so a glyph drawn much larger than the app
//!   draws glyphs anywhere else cannot be made lighter without a second copy of
//!   it. The composer's action row is that place: it letters its icons at
//!   1.25rem where the rest of the app draws them at about 0.75rem, and the
//!   upstream stroke of 2 reads as a marker pen at that size. The library's own
//!   copies stay in use at every other call site -- so the app carries two
//!   weights split by *how big a glyph is drawn*, which is a rule, rather than
//!   by which glyph it is, which would be taste. A third weight, or a `-light`
//!   entry whose call site is not drawn oversized, is the door reopening.
//!   Checked in through the manifest, whose sync script rewrites the stroke.
//!
//! A variant here is a name the window serves, so naming a glyph nobody picked
//! fails to build rather than drawing nothing.
//!
//! The cost of leaning on the library's names is real and worth naming, since
//! it is paid silently: the library **renames icons when it packages them**
//! (its `close.svg` is Lucide's `x`, its `dash` is `minus`, and its `delete` is
//! the backspace key rather than a waste bin), those names carry no guarantee
//! across the pinned revision being bumped, and an icon that fails to resolve
//! draws nothing at all rather than failing to build. Bumping the pin means
//! looking at the app's chrome afterwards.
//!
//! Checked-in assets are served under an `onehand/` prefix so they can never
//! shadow the bundled set at bare `icons/…`, which the library's own components
//! reach for in ~97 places.

use gpui::SharedString;
use gpui_component::IconNamed;

/// Path prefix for onehand's own assets inside the merged [`crate::assets`]
/// source. Must not collide with the bundled library set at `icons/`.
pub const PREFIX: &str = "onehand/icons/";

macro_rules! icons {
    (
        picked: [$($picked:ident),* $(,)?],
        checked_in: [$($variant:ident => $file:literal),* $(,)?] $(,)?
    ) => {
        gpui_kit_assets::icon_assets!(pub Picked, [$($picked),*]);

        /// A compile-time identifier for one icon the window serves beyond the
        /// default bundle.
        ///
        /// `dead_code` is allowed for the whole enum on purpose: this is a
        /// *registry*, and its checked-in half mirrors
        /// `assets/icons/manifest.toml` exactly — which is what
        /// `registry_and_manifest_have_the_same_assets` asserts. A variant with
        /// no call site is a curated asset nobody has needed yet, not a mistake;
        /// dropping it to satisfy the lint would break the mirror the test
        /// checks and leave a shipped SVG unreachable.
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        #[allow(dead_code, reason = "a registry mirroring manifest.toml")]
        pub enum Icon {
            $($picked,)*
            $($variant),*
        }

        impl Icon {
            /// Every registered icon, used by the consistency tests.
            #[allow(dead_code, reason = "read by the registry consistency tests")]
            pub const ALL: &'static [Icon] = &[$(Icon::$picked,)* $(Icon::$variant),*];

            /// The checked-in asset name, shared with the source manifest, or
            /// `None` for a glyph picked from the kit.
            #[cfg(test)]
            pub const fn asset_name(self) -> Option<&'static str> {
                match self {
                    $(Icon::$picked => None,)*
                    $(Icon::$variant => Some($file)),*
                }
            }
        }

        impl IconNamed for Icon {
            fn path(self) -> SharedString {
                match self {
                    $(Icon::$picked => gpui_kit_assets::IconName::$picked.path(),)*
                    $(Icon::$variant => SharedString::from(concat!("onehand/icons/", $file, ".svg"))),*
                }
            }
        }

        /// Resolve a checked-in asset path back to its embedded bytes, for
        /// [`crate::assets`].
        pub fn embedded(path: &str) -> Option<&'static [u8]> {
            match path.strip_prefix(PREFIX)?.strip_suffix(".svg")? {
                $($file => Some(include_bytes!(
                    concat!("../../../assets/icons/", $file, ".svg")
                ))),*,
                _ => None,
            }
        }

        /// Every checked-in path this module serves, for `AssetSource::list`.
        pub fn all_paths() -> Vec<SharedString> {
            vec![$(SharedString::from(concat!("onehand/icons/", $file, ".svg"))),*]
        }
    };
}

icons! {
    picked: [
        // The transcript's "Changed" activity group. The bundled set has no
        // pencil of any kind, and its nearest shape is `replace` — a
        // find-and-replace mark, which reads as swapping one box for another
        // rather than as editing a file.
        SquarePen,
        // Splitting a project onto its own branch, in the project row's menu.
        // The bundled set holds no branch, fork or split of any kind — its only
        // git-adjacent drawing is the GitHub brand mark, which names a website
        // rather than the operation.
        GitBranch,
        // A prompt's *Fork from here*, which carries the conversation on in a
        // new session from that point and leaves the original as it is. The
        // branch mark above already means cutting a project onto a branch of
        // its own; the fork mark is the conversation splitting in two.
        GitFork,
        // Staging a file onto the prompt being written. The bundled set holds
        // no clip, pin or paperclip of any kind; the nearest thing it draws is
        // `inbox`, a tray that says mail has arrived — the opposite direction
        // from handing a file to the agent.
        Paperclip,
        // The composer's *Mention a file* menu entry, which inserts an `@`. The
        // bundled set has no at-sign; the nearest mark it draws is `asterisk`,
        // which is a different character, and a button whose whole job is to
        // say which character it types cannot draw another one. This button is
        // also the only route to a mention for anyone whose input method
        // swallows the typed character.
        AtSign,
        // The composer's slash-command button, which inserts a `/`. The bundled
        // set has no slash; its `dash` is a minus sign. Lucide's `command` is
        // not this either — that is the ⌘ modifier key. The enclosed form and
        // not the bare `slash`: at icon size a lone diagonal stroke has no
        // weight beside the paperclip and the at-sign in the same menu, and
        // reads as a separator between the entries.
        SquareSlash,
        // The permission mode's chip under the composer leads with the mark of
        // the setting it belongs to. The bundled set draws no shield, lock or
        // badge of any kind — its `circle-check` says an answer was correct,
        // not that permission is being asked for.
        Shield,
        // The conversation header's *Close session*, which ends a session while
        // keeping every word of it. Every mark the bundled set has for ending
        // something is an × — which says "dismiss this", and beside two dock
        // toggles reads as closing a panel. The exit-door arrow is the mark apps
        // use for ending a session, and unlike a power mark it does not read as
        // quitting the whole app.
        LogOut,
        // The Shortcuts entry in Settings' nav. The bundled set draws no
        // keyboard or key of any kind; its nearest shapes are `square-terminal`,
        // the terminal the app already has a dock for, and `asterisk`.
        Keyboard,
        // The rail's *Issues* row, which opens the Issues page. The bundled set
        // draws no issue mark: its nearest shapes are `circle-check`, which says
        // something passed, and `inbox`, which the *Tasks* row already wears.
        CircleDot,
        // The Issues page's *Refresh*. The bundled set draws no refresh mark:
        // its nearest shapes are `redo`, which reads as stepping an edit
        // forward, and `loader-circle`, which already means running.
        RefreshCw,
        // The rail's status marks and row actions. The bundled set draws no
        // hand, no bare circle, no archive box, no stop square and no filter:
        // its nearest shapes are `circle-check` and `circle-x`, which already
        // mean done and dismiss; `pause`, which says the turn would resume; and
        // `inbox`, which the Tasks row wears. Needs input is a raised hand, idle
        // an empty circle, Stop a square, Archive a box, and the sessions filter
        // the filter lines.
        Hand,
        Circle,
        // The header's button to a lead's workers. The bundled set draws no
        // people of any kind; its nearest, `bot`, already marks an agent that
        // controls onehand.
        Users,
        Archive,
        Square,
        ListFilter,
        // The composer's *Fast* control, drawn as its mark alone and lit while
        // fast mode is on. The bundled set draws no bolt, zap or flash: its
        // nearest shapes are `loader-circle`, which already means running, and
        // `star`, which reads as a favourite.
        Zap,
    ],
    checked_in: [
        ArrowUpLight => "arrow-up-light",
        PlusLight => "plus-light",
    ],
}

#[cfg(test)]
mod tests {
    use super::{Icon, embedded};
    use gpui_component::IconNamed as _;

    /// A feature crate names the app's marks by a path of its own, since it cannot
    /// reach this registry; each has to be the path served here.
    #[test]
    fn a_feature_crate_names_an_app_icon_by_the_path_it_is_served_at() {
        use onehand_ui::AppIcon;
        assert_eq!(AppIcon::CircleDot.path(), Icon::CircleDot.path());
        assert_eq!(AppIcon::Refresh.path(), Icon::RefreshCw.path());
    }

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
            .filter_map(|icon| icon.asset_name())
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
                .find(|icon| icon.asset_name() == Some(name.as_str()))
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
        for name in Icon::ALL.iter().filter_map(|icon| icon.asset_name()) {
            let path = format!("onehand/icons/{name}.svg");
            assert!(
                embedded(&path).is_some_and(|bytes| !bytes.is_empty()),
                "{name} must be embedded at {path}"
            );
        }
        // A bundled library path must fall through to gpui-kit-assets.
        assert!(embedded("icons/folder.svg").is_none());
    }

    /// Every icon loads through the window's asset source: a picked glyph the
    /// kit does not embed would draw nothing rather than fail.
    #[test]
    fn every_icon_loads_through_the_window_assets() {
        use gpui::AssetSource as _;
        for icon in Icon::ALL.iter().copied() {
            let path = icon.path();
            assert!(
                crate::assets::Assets
                    .load(&path)
                    .ok()
                    .flatten()
                    .is_some_and(|bytes| !bytes.is_empty()),
                "{icon:?} must load at {path}"
            );
        }
    }
}
