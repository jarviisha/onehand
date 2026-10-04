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
    toml::from_str(text).map_err(|err| err.message().to_string())
}

/// Every template file in `dir`, each read or why it could not be, in file
/// name order. Blocking.
pub fn load_all_blocking(dir: &Path) -> Vec<(PathBuf, Result<Template, String>)> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<_> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|x| x == "toml"))
        .map(|path| {
            let read = std::fs::read_to_string(&path)
                .map_err(|err| err.to_string())
                .and_then(|text| parse(&text));
            (path, read)
        })
        .collect();
    found.sort_by(|a, b| a.0.cmp(&b.0));
    found
}

/// Serializes every read-check-write here, so two saves cannot interleave.
static WRITING: Mutex<()> = Mutex::new(());

/// Write `template` to the file `at`, or to a new file in `dir` named for it
/// when `at` is `None`. Hands back the file it is in. Blocking.
///
/// A file at `at` that this build cannot read is refused rather than
/// written over.
pub fn save_blocking(
    dir: &Path,
    at: Option<&Path>,
    template: &Template,
) -> Result<PathBuf, String> {
    let _held = WRITING
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let path = match at {
        Some(path) => {
            match std::fs::read_to_string(path) {
                Ok(text) => {
                    parse(&text)
                        .map_err(|why| format!("{} is kept as it is: {why}", path.display()))?;
                }
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(err) => return Err(format!("{} could not be read: {err}", path.display())),
            }
            path.to_path_buf()
        }
        None => free_path(dir, &slug(&template.name)),
    };
    let text = toml::to_string_pretty(template).map_err(|err| err.to_string())?;
    crate::config::write_atomic(&path, &text)
        .map_err(|err| format!("{} could not be written: {err}", path.display()))?;
    Ok(path)
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
/// behind. Blocking.
///
/// Safe to run at every start and again after a crash part way: a file this
/// build cannot read stays where it is, a name already in `new` keeps the
/// copy there, and each file is written in full before its old one goes.
/// Anything that is not a template is left alone, and `old` goes once empty.
pub(crate) fn migrate_blocking(old: &Path, new: &Path) -> Vec<String> {
    let mut problems = Vec::new();
    let Ok(entries) = std::fs::read_dir(old) else {
        return problems;
    };
    for path in entries.filter_map(Result::ok).map(|entry| entry.path()) {
        if path.extension().is_none_or(|x| x != "toml") {
            continue;
        }
        let Some(name) = path.file_name() else {
            continue;
        };
        let to = new.join(name);
        let moved = std::fs::read_to_string(&path)
            .map_err(|err| err.to_string())
            .and_then(|text| parse(&text).map(|_| text))
            .and_then(|text| match to.exists() {
                true => Ok(()),
                false => crate::config::write_atomic(&to, &text).map_err(|e| e.to_string()),
            })
            .and_then(|()| std::fs::remove_file(&path).map_err(|e| e.to_string()));
        if let Err(why) = moved {
            problems.push(format!("{} was not moved: {why}", path.display()));
        }
    }
    let _ = std::fs::remove_dir(old);
    problems
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
