//! `[font]`, and choosing a monospace family the machine actually has.

use serde::{Deserialize, Serialize};

/// `[font]` — the monospace family to prefer, if the machine has one by that
/// name.
///
/// One key, because one key is what is read. This table also carried a body
/// size, a master zoom, a sans family and a fallback list; all four parsed
/// cleanly and none of them reached the screen, so a file setting `size = 18`
/// loaded without complaint and changed nothing. A key that is honoured only by
/// the parser is worse than a missing one: the missing key is a feature the app
/// does not have, and the parsed one is a feature it appears to have and lies
/// about.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FontConfig {
    pub monospace: Option<String>,
}

/// Monospace families to fall back through, in order, when nothing preferred
/// is installed.
///
/// Every platform's list in one, because only the installed ones can match:
/// picking by `cfg!(target_os)` would be guessing at the same thing the caller
/// can simply look up.
const MONO_FALLBACKS: &[&str] = &[
    // Platform defaults first — the family a user of this OS expects to see,
    // before the one a developer happened to install.
    "SF Mono",
    "Menlo",
    "Monaco",
    "Cascadia Mono",
    "Consolas",
    "DejaVu Sans Mono",
    "Liberation Mono",
    "Noto Sans Mono",
    "Ubuntu Mono",
    "Adwaita Mono",
    // Then the ones people go out of their way to install.
    "JetBrains Mono",
    "Fira Code",
    "Source Code Pro",
    "Cascadia Code",
    "Courier New",
];

/// The monospace family to actually ask for, given what is installed.
///
/// **A font family is a request, not a guarantee, and a missing one fails
/// silently** — the text renders in whatever the fallback face is and nothing
/// says why. That is not hypothetical: the component library's default mono
/// family is one name per platform, and on Linux it is DejaVu Sans Mono, which
/// plenty of distributions do not ship. Every diff, every command, every line
/// of terminal output then draws in the body face, and the code asking for
/// mono looks correct while the screen says otherwise.
///
/// So the family is chosen against the list of what the system actually has.
/// `preferred` is tried in order — the user's configured choice, then whatever
/// default was already in place — then the fallback ladder, and last **any**
/// installed family whose name says monospace, sorted so the answer is the
/// same on every launch. `None` means the machine offered nothing that
/// identifies itself as monospace, in which case the caller has no better move
/// than to leave the default alone.
///
/// Matching is case-insensitive because font enumeration capitalizes as the
/// foundry pleases and a config file is typed by a person.
pub fn resolve_monospace<'a>(
    preferred: impl IntoIterator<Item = &'a str>,
    installed: &[String],
) -> Option<String> {
    let find = |want: &str| {
        installed
            .iter()
            .find(|have| have.eq_ignore_ascii_case(want))
            .cloned()
    };

    preferred
        .into_iter()
        .filter(|want| !want.trim().is_empty())
        .find_map(&find)
        .or_else(|| MONO_FALLBACKS.iter().copied().find_map(find))
        .or_else(|| {
            let mut named: Vec<&String> = installed
                .iter()
                .filter(|have| have.to_ascii_lowercase().contains("mono"))
                .collect();
            named.sort();
            named.first().map(|name| name.to_string())
        })
}
