//! `path:line:col` token parsing. Pure and GUI-free, so link detection and the
//! "open in editor" affordance can be unit-tested without a window.
//!
//! ⚠️ **Nothing calls this yet.** It is the parser for clickable `path:line:col`
//! tokens in agent prose, and the transcript does not scan for them — it renders
//! prose through `TextView::markdown`. Kept because it
//! is a tested pure function the feature needs unchanged, but it is *staged*,
//! not live: do not read its presence as evidence the feature exists.

/// Resolve a path an agent named against the session's project root.
///
/// ACP requires absolute paths, but "the peer follows the spec" is not
/// something to bet a file open on: a relative path handed straight to the
/// filesystem resolves against the *process* working directory, which is
/// whatever shell launched onehand — not the project the agent is working in.
/// With several roots open, or the app started from anywhere but the root, that
/// opens the wrong file or none at all.
///
/// Absolute paths pass through untouched, so a conforming agent is unaffected.
/// No canonicalization and no touching the disk: this is a pure join, and the
/// caller is already prepared for the file not to exist.
pub fn resolve_in_root(root: &std::path::Path, path: &str) -> std::path::PathBuf {
    let path = std::path::Path::new(path);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

/// A parsed file location reference.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathLoc {
    pub path: String,
    pub line: Option<u32>,
    pub col: Option<u32>,
}

/// Parse a `path`, `path:line`, or `path:line:col` token. The path may itself
/// contain colons only if they are not a trailing numeric `:line[:col]` tail,
/// so we peel digits off the right.
///
/// Only a token whose path part *looks like a path* (contains `/`, `\` or
/// `.`) counts — `localhost:8080` is a host:port, not the file `localhost`
/// at line 8080, and returning `None` lets the caller fall back to its
/// URL behavior. A numeric segment must also actually fit a `u32`: an
/// overflowing tail (`file:99999999999999`) is not a line number and must
/// not be silently stripped off the path.
pub fn parse_path_line(token: &str) -> Option<PathLoc> {
    let token = token.trim();
    if token.is_empty() {
        return None;
    }

    // Split into colon-separated parts and peel a numeric tail of up to 2.
    let parts: Vec<&str> = token.split(':').collect();
    let as_num = |s: &str| -> Option<u32> {
        if !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) {
            s.parse().ok()
        } else {
            None
        }
    };
    let pathish = |s: &str| s.contains('/') || s.contains('\\') || s.contains('.');

    // Try col + line, then just line, then no numbers.
    if parts.len() >= 3 {
        if let (Some(line), Some(col)) = (
            as_num(parts[parts.len() - 2]),
            as_num(parts[parts.len() - 1]),
        ) {
            let path = parts[..parts.len() - 2].join(":");
            if pathish(&path) {
                return Some(PathLoc {
                    path,
                    line: Some(line),
                    col: Some(col),
                });
            }
        }
    }
    if parts.len() >= 2 {
        if let Some(line) = as_num(parts[parts.len() - 1]) {
            let path = parts[..parts.len() - 1].join(":");
            if pathish(&path) {
                return Some(PathLoc {
                    path,
                    line: Some(line),
                    col: None,
                });
            }
        }
    }
    pathish(token).then(|| PathLoc {
        path: token.to_string(),
        line: None,
        col: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_relative_path_resolves_against_the_root_not_the_process_cwd() {
        let root = std::path::Path::new("/home/me/project");
        assert_eq!(
            resolve_in_root(root, "src/main.rs"),
            std::path::PathBuf::from("/home/me/project/src/main.rs")
        );
        // A conforming agent sends absolute paths, and those must not be
        // re-rooted into nonsense like /home/me/project/home/me/other/x.rs.
        assert_eq!(
            resolve_in_root(root, "/home/me/other/x.rs"),
            std::path::PathBuf::from("/home/me/other/x.rs")
        );
    }

    #[test]
    fn plain_path() {
        assert_eq!(
            parse_path_line("src/main.rs"),
            Some(PathLoc {
                path: "src/main.rs".into(),
                line: None,
                col: None
            })
        );
    }

    #[test]
    fn path_with_line() {
        assert_eq!(
            parse_path_line("src/main.rs:42"),
            Some(PathLoc {
                path: "src/main.rs".into(),
                line: Some(42),
                col: None
            })
        );
    }

    #[test]
    fn path_with_line_and_col() {
        assert_eq!(
            parse_path_line("src/main.rs:42:7"),
            Some(PathLoc {
                path: "src/main.rs".into(),
                line: Some(42),
                col: Some(7)
            })
        );
    }

    #[test]
    fn windows_drive_colon_is_kept() {
        // A leading drive colon is not a numeric tail, so it stays in the path.
        assert_eq!(
            parse_path_line("C:/proj/main.rs:10"),
            Some(PathLoc {
                path: "C:/proj/main.rs".into(),
                line: Some(10),
                col: None
            })
        );
    }

    #[test]
    fn empty_is_none() {
        assert_eq!(parse_path_line("  "), None);
    }

    #[test]
    fn host_port_is_not_a_file() {
        // `localhost:8080` must fall through to the caller's URL handling,
        // not open the editor on a file named "localhost".
        assert_eq!(parse_path_line("localhost:8080"), None);
        assert_eq!(parse_path_line("localhost"), None);
        // …but a dotted host-like *path* still parses (it may be a real file).
        assert_eq!(
            parse_path_line("notes.txt:12"),
            Some(PathLoc {
                path: "notes.txt".into(),
                line: Some(12),
                col: None
            })
        );
    }

    #[test]
    fn overflowing_tail_stays_in_the_path() {
        // A tail that doesn't fit u32 is not a line number — it must not be
        // stripped off the path (the old code dropped it and kept line=None).
        assert_eq!(
            parse_path_line("file.log:99999999999999"),
            Some(PathLoc {
                path: "file.log:99999999999999".into(),
                line: None,
                col: None
            })
        );
        assert_eq!(
            parse_path_line("src/a.rs:4294967296"), // u32::MAX + 1
            Some(PathLoc {
                path: "src/a.rs:4294967296".into(),
                line: None,
                col: None
            })
        );
    }
}
