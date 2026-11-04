//! The host aliases the user's ssh config names, to offer when adding a
//! project on a host. Only plain `Host` names count; ssh itself resolves them
//! when it connects, so nothing here needs the rest of the grammar.

use std::fs;
use std::path::{Path, PathBuf};

/// How deep `Include` is followed; a file that includes itself ends here.
const INCLUDE_DEPTH: usize = 8;

/// `~/.ssh`, where ssh keeps its config.
pub fn ssh_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".ssh"))
}

/// Every plain `Host` name in `ssh_dir/config` and what it includes, in file
/// order and once each. A missing or unreadable file offers none.
pub fn aliases(ssh_dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    read(ssh_dir, &ssh_dir.join("config"), 0, &mut out);
    out
}

fn read(ssh_dir: &Path, file: &Path, depth: usize, out: &mut Vec<String>) {
    if depth > INCLUDE_DEPTH {
        return;
    }
    let Ok(text) = fs::read_to_string(file) else {
        return;
    };
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        let (key, rest) = match line.split_once(|c: char| c.is_whitespace() || c == '=') {
            Some((k, r)) => (
                k,
                r.trim_start_matches(|c: char| c.is_whitespace() || c == '='),
            ),
            None => continue,
        };
        if key.eq_ignore_ascii_case("host") {
            for name in rest.split_whitespace() {
                let plain = !name.contains(['*', '?', '!']);
                if plain && !out.iter().any(|n| n == name) {
                    out.push(name.to_string());
                }
            }
        } else if key.eq_ignore_ascii_case("include") {
            for pattern in rest.split_whitespace() {
                for path in expand(ssh_dir, pattern) {
                    read(ssh_dir, &path, depth + 1, out);
                }
            }
        }
    }
}

/// The files an `Include` pattern names: `~/` is the home, a relative path
/// sits under the ssh dir, and a `*` in the file name matches within that one
/// directory, sorted.
fn expand(ssh_dir: &Path, pattern: &str) -> Vec<PathBuf> {
    let path = match pattern.strip_prefix("~/") {
        Some(rest) => match dirs::home_dir() {
            Some(home) => home.join(rest),
            None => return Vec::new(),
        },
        None => ssh_dir.join(pattern),
    };
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return Vec::new();
    };
    if !name.contains('*') {
        return vec![path];
    }
    let Ok(entries) = fs::read_dir(path.parent().unwrap_or(ssh_dir)) else {
        return Vec::new();
    };
    let mut found: Vec<PathBuf> = entries
        .flatten()
        .filter(|e| e.file_name().to_str().is_some_and(|n| star_match(name, n)))
        .map(|e| e.path())
        .collect();
    found.sort();
    found
}

/// Whether `text` fits `pattern`, where each `*` stands for any run of characters.
fn star_match(pattern: &str, text: &str) -> bool {
    let mut parts = pattern.split('*');
    let first = parts.next().unwrap_or("");
    let Some(mut rest) = text.strip_prefix(first) else {
        return false;
    };
    let mut parts: Vec<&str> = parts.collect();
    let last = parts.pop().unwrap_or("");
    for mid in parts {
        match rest.find(mid) {
            Some(i) => rest = &rest[i + mid.len()..],
            None => return false,
        }
    }
    rest.ends_with(last)
}

#[cfg(test)]
mod tests;
