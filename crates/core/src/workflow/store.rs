//! The person's own templates: one TOML file each in
//! `<config_dir>/onehand/workflows/`.
//!
//! **A file this build cannot read is never written over.** One carrying a
//! newer schema was written by a newer onehand, and one that does not parse
//! is somebody's template with a typo in it; replacing either would lose it.

use super::template::{Template, SCHEMA_VERSION};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// `<config_dir>/onehand/workflows/`.
pub fn dir() -> PathBuf {
    crate::config::config_dir().join("workflows")
}

/// Read a template from its file's text.
pub fn parse(text: &str) -> Result<Template, String> {
    #[derive(Deserialize)]
    struct Version {
        schema_version: u32,
    }
    let version: Version = toml::from_str(text)
        .map_err(|err| format!("it has no schema_version: {}", err.message()))?;
    if version.schema_version > SCHEMA_VERSION {
        return Err(format!(
            "it was written by a newer onehand (template schema {}; this build reads up to \
             {SCHEMA_VERSION})",
            version.schema_version
        ));
    }
    let template: Template = toml::from_str(text).map_err(|err| err.message().to_string())?;
    // A key onehand does not read is most likely a typo, and a save would
    // drop it without a word; serde's own refusal does not reach into the
    // flattened step kinds.
    let given: toml::Value = toml::from_str(text).map_err(|err| err.message().to_string())?;
    let read = toml::Value::try_from(&template).map_err(|err| err.to_string())?;
    match unread_key(&given, &read, "") {
        Some(key) => Err(format!("it has a key `{key}` that onehand does not read")),
        None => Ok(template),
    }
}

/// The first key in `given` that `read` lacks, by its path.
fn unread_key(given: &toml::Value, read: &toml::Value, at: &str) -> Option<String> {
    match (given, read) {
        (toml::Value::Table(given), toml::Value::Table(read)) => {
            given.iter().find_map(|(key, value)| {
                let path = match at.is_empty() {
                    true => key.clone(),
                    false => format!("{at}.{key}"),
                };
                match read.get(key) {
                    Some(there) => unread_key(value, there, &path),
                    None => Some(path),
                }
            })
        }
        (toml::Value::Array(given), toml::Value::Array(read)) => given
            .iter()
            .zip(read)
            .enumerate()
            .find_map(|(i, (value, there))| unread_key(value, there, &format!("{at}[{i}]"))),
        _ => None,
    }
}

/// The template in the file `path`, its id the file's name when the file
/// has none, as one written before ids does not. Blocking.
pub fn read_blocking(path: &Path) -> Result<Template, String> {
    let text = std::fs::read_to_string(path).map_err(|err| err.to_string())?;
    let mut template = parse(&text)?;
    if template.id.is_empty() {
        template.id = stem(path);
    }
    Ok(template)
}

fn stem(path: &Path) -> String {
    path.file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Every template file in `dir`, each read or why it could not be, in file
/// name order. Blocking.
///
/// `ponytail:` a file copied by hand keeps its id, so two may share one;
/// Retry offers the first. Give each a fresh id on load if that bites.
pub fn load_all_blocking(dir: &Path) -> Vec<(PathBuf, Result<Template, String>)> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<_> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|x| x == "toml"))
        .map(|path| {
            let read = read_blocking(&path);
            (path, read)
        })
        .collect();
    found.sort_by(|a, b| a.0.cmp(&b.0));
    found
}

/// Serializes every read-check-write here, so two saves cannot interleave.
static WRITING: Mutex<()> = Mutex::new(());

/// Write `template` to the file `at`, or to a new file in `dir` named for it
/// when `at` is `None`. Hands back the file it is in and the template as
/// written. Blocking.
///
/// The id and version are this function's to set, whatever `template`
/// carries: a new file takes its file name as id at version 1, and an
/// existing one keeps its id, its version going up by one when what it says
/// changed. A file at `at` that this build cannot read is refused rather
/// than written over.
pub fn save_blocking(
    dir: &Path,
    at: Option<&Path>,
    template: &Template,
) -> Result<(PathBuf, Template), String> {
    let _held = WRITING
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut template = template.clone();
    let path = at.map_or_else(|| free_path(dir, &slug(&template.name)), Path::to_path_buf);
    let before = match at.map(|path| (path, std::fs::metadata(path))) {
        Some((path, Ok(_))) => Some(
            read_blocking(path)
                .map_err(|why| format!("{} is kept as it is: {why}", path.display()))?,
        ),
        Some((_, Err(err))) if err.kind() != std::io::ErrorKind::NotFound => {
            return Err(format!("{} could not be read: {err}", path.display()));
        }
        _ => None,
    };
    (template.id, template.version) = match before {
        Some(before) if before.same_content(&template) => (before.id, before.version),
        Some(before) => (before.id, before.version + 1),
        None => (stem(&path), 1),
    };
    let text = toml::to_string_pretty(&template).map_err(|err| err.to_string())?;
    crate::config::write_atomic(&path, &text)
        .map_err(|err| format!("{} could not be written: {err}", path.display()))?;
    Ok((path, template))
}

/// Write `template` to `path`, a file of the person's choosing anywhere.
/// Blocking.
pub fn export_blocking(path: &Path, template: &Template) -> Result<(), String> {
    let text = toml::to_string_pretty(template).map_err(|err| err.to_string())?;
    crate::config::write_atomic(path, &text)
        .map_err(|err| format!("{} could not be written: {err}", path.display()))
}

/// Delete the template file `path`. Blocking.
pub fn delete_blocking(path: &Path) -> Result<(), String> {
    let _held = WRITING
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    match std::fs::remove_file(path) {
        Err(err) if err.kind() != std::io::ErrorKind::NotFound => {
            Err(format!("{} could not be deleted: {err}", path.display()))
        }
        _ => Ok(()),
    }
}

/// Move the templates a build from before the rename kept in `pipelines/`
/// to [`dir`], and say what was left behind. Blocking.
pub fn migrate_old_dir_blocking() -> Vec<String> {
    migrate_blocking(&crate::config::config_dir().join("pipelines"), &dir())
}

/// Move every template file from `old` to `new`, and say what was left
/// behind: a file this build cannot read stays, and a name already in `new`
/// keeps the copy there, the old one going only when it is the same text.
/// Blocking.
pub(crate) fn migrate_blocking(old: &Path, new: &Path) -> Vec<String> {
    crate::config::migrate_dir_blocking(
        old,
        new,
        "toml",
        |text| parse(text).map(|_| text.to_string()),
        |there, text| there == text,
    )
}

/// A file name from a template's name: lowercase ASCII letters, digits and
/// single dashes, `workflow` when nothing of the name survives.
pub(crate) fn slug(name: &str) -> String {
    let mut slug = String::new();
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = slug.trim_matches('-');
    match slug.is_empty() {
        true => "workflow".to_string(),
        false => slug.to_string(),
    }
}

/// `<dir>/<slug>.toml`, or the first of `<slug>-2.toml`, `<slug>-3.toml` …
/// that no file holds yet.
fn free_path(dir: &Path, slug: &str) -> PathBuf {
    std::iter::once(slug.to_string())
        .chain((2..).map(|n| format!("{slug}-{n}")))
        .map(|name| dir.join(format!("{name}.toml")))
        .find(|path| !path.exists())
        .unwrap_or_else(|| dir.join(format!("{slug}.toml")))
}
