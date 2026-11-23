//! The words each step of a build says to a host, in the host's own shell.
//!
//! ssh hands a line to the account's shell: a POSIX one on Linux, `cmd.exe`
//! on Windows. The steps of a build are the same on both; only how a word is
//! quoted and how each step is spelled differ, and that is all this holds.

use std::path::{Path, PathBuf};

use super::{MARK, RUN_PID};
use crate::config::HostOs;
use crate::host::ssh::CMD_NOT_FOUND;

/// The shell a host's ssh hands a line to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Shell {
    Posix,
    /// Windows' `cmd.exe`, the shell its OpenSSH starts unless changed.
    Cmd,
}

/// Why a routed command, which brings back only what its run wrote, does not
/// build on a Windows host: nothing there marks when the run began.
const ROUTED_ON_LINUX: &str = "routed commands build on Linux hosts only";

/// The exit code a POSIX shell gives a command it could not find.
const POSIX_NOT_FOUND: i32 = 127;

/// The exit code of [`Shell::init`] when the host lacks a program a build
/// needs, each named on stderr.
pub(super) const NEEDS: i32 = 4;

/// The exit code of [`Shell::send`] when a path was not there, named on
/// stderr.
pub(super) const NOT_BUILT: i32 = 3;

impl Shell {
    /// A host declared as Windows is told in `cmd.exe`'s words; every other,
    /// a host nobody declared among them, in a POSIX shell's.
    pub(crate) fn of(os: &HostOs) -> Self {
        match os {
            HostOs::Windows => Self::Cmd,
            HostOs::Linux | HostOs::Other(_) => Self::Posix,
        }
    }

    /// `text` as one word of a line, or why it cannot be one there.
    ///
    /// `cmd.exe` expands `%` even inside quotes and has no way to write a
    /// `"` inside them, so a word holding either is refused rather than sent
    /// as something else.
    pub(crate) fn word(self, text: &str) -> Result<String, String> {
        match self {
            Self::Posix => Ok(format!("'{}'", text.replace('\'', r"'\''"))),
            // A line ends at a line break, whatever quotes it stands in.
            Self::Cmd if text.contains(|c: char| c.is_control()) => Err(format!(
                "{} cannot be built on a Windows host: its name holds a line break",
                text.escape_debug()
            )),
            Self::Cmd if text.contains(['%', '"']) => Err(format!(
                "{text} cannot be built on a Windows host: its name holds % or \""
            )),
            Self::Cmd => Ok(format!("\"{text}\"")),
        }
    }

    /// Make the folder `dir`, and the repository in it, or exit [`NEEDS`]
    /// naming on stderr which of `git` and `tar` the host lacks: asked first,
    /// so a host that cannot build says so before any code is sent to it. On
    /// Windows the repository is told to leave line endings alone and to
    /// take long paths, so the code is built there as it is here.
    pub(crate) fn init(self, dir: &str) -> Result<String, String> {
        let dir = self.word(dir)?;
        Ok(match self {
            Self::Posix => format!(
                "m=; for t in git tar; do command -v $t >/dev/null 2>&1 || m=\"$m $t\"; done; \
                 test -z \"$m\" || {{ printf %s \"$m\" >&2; exit {NEEDS}; }}; \
                 mkdir -p {dir} && git -C {dir} init -q"
            ),
            Self::Cmd => format!(
                "(where git >nul 2>nul || (echo git>&2 & exit {NEEDS})) && \
                 (where tar >nul 2>nul || (echo tar>&2 & exit {NEEDS})) && \
                 git init -q {dir} && git -C {dir} config core.autocrlf false && \
                 git -C {dir} config core.longpaths true"
            ),
        })
    }

    /// In `dir`, check the snapshot `sha` out, go to `within`, print the
    /// mark, and run `command` with what it says on stderr among its lines.
    ///
    /// The command is the person's own line for that shell: run by the login
    /// shell on a POSIX host, so a toolchain a profile puts on `PATH` is
    /// found, in a session of its own whose id is written to [`RUN_PID`], so
    /// a stopped run can end it and all it started ([`Shell::stop`]), and by a `cmd.exe` of its own on Windows, so the whole line's
    /// stderr is folded in, not its last command's.
    ///
    /// **On Windows the line is read twice**, by the `cmd.exe` ssh starts and
    /// by the one that runs the command, and quotes there toggle rather than
    /// nest: a `|` inside the command's own quotes would be a pipe to the
    /// first. So every character the first would act on is escaped
    /// ([`for_outer_cmd`]), and the second is handed the line as written.
    ///
    /// With `mark`, the file of that name in `dir` is touched just before
    /// the command, so what it writes is what is newer.
    pub(crate) fn run(
        self,
        dir: &str,
        sha: &str,
        within: &Path,
        command: &str,
        mark: Option<&str>,
    ) -> Result<String, String> {
        let touch = match (self, mark) {
            (_, None) => String::new(),
            (Self::Posix, Some(mark)) => format!("touch {} && ", self.word(mark)?),
            (Self::Cmd, Some(_)) => return Err(ROUTED_ON_LINUX.to_string()),
        };
        let dir = self.word(dir)?;
        let within = match within.as_os_str().is_empty() {
            true => String::new(),
            false => format!("cd {} && ", self.word(&within.display().to_string())?),
        };
        Ok(match self {
            Self::Posix => format!(
                "cd {dir} && r=$PWD && rm -f {RUN_PID} && git checkout -q -f --detach {sha} && \
                 {touch}{within}echo {MARK} && {{ setsid \"${{SHELL:-/bin/sh}}\" -lc {} 2>&1 & \
                 echo $! > \"$r/{RUN_PID}\"; wait $!; }}",
                self.word(command)?
            ),
            Self::Cmd => format!(
                "cd /d {dir} && git checkout -q -f --detach {sha} && {within}echo {MARK} && \
                 cmd /d /s /c ^\"{}^\" 2>&1",
                for_outer_cmd(command)
            ),
        })
    }

    /// In `dir`, send `paths` as a tar on stdout, or exit [`NOT_BUILT`] naming on stderr
    /// the first that is not there, before anything is sent.
    pub(crate) fn send(self, dir: &str, paths: &[PathBuf]) -> Result<String, String> {
        let dir = self.word(dir)?;
        let paths = paths
            .iter()
            .map(|path| self.word(&path.display().to_string()))
            .collect::<Result<Vec<_>, _>>()?
            .join(" ");
        Ok(match self {
            Self::Posix => format!(
                "cd {dir} && for p in {paths}; do test -e \"$p\" || {{ printf %s \"$p\" >&2; \
                 exit {NOT_BUILT}; }}; done && tar -cf - -- {paths}"
            ),
            Self::Cmd => format!(
                "cd /d {dir} && (for %p in ({paths}) do @if not exist %p (echo %~p>&2 & exit {NOT_BUILT})) \
                 && tar -cf - -- {paths}"
            ),
        })
    }

    /// In `dir`, name on stdout each of `paths` holding something newer
    /// than the file `mark`, each ended by a NUL and the list by an empty
    /// one, then send those as a tar. A path not there is simply not named.
    pub(crate) fn send_fresh(
        self,
        dir: &str,
        paths: &[PathBuf],
        mark: &str,
    ) -> Result<String, String> {
        let dir = self.word(dir)?;
        let mark = self.word(mark)?;
        let paths = paths
            .iter()
            .map(|path| self.word(&path.display().to_string()))
            .collect::<Result<Vec<_>, _>>()?
            .join(" ");
        match self {
            Self::Posix => Ok(format!(
                "cd {dir} || exit 1; set --; for p in {paths}; do \
                 test -n \"$(find \"./$p\" -newer {mark} -print -quit 2>/dev/null)\" && \
                 set -- \"$@\" \"$p\"; done; for p in \"$@\"; do printf '%s\\0' \"$p\"; done; \
                 printf '\\0'; test $# -eq 0 || exec tar -cf - -- \"$@\""
            )),
            Self::Cmd => Err(ROUTED_ON_LINUX.to_string()),
        }
    }

    /// In `dir`, end the command a stopped [`Shell::run`] left running, and
    /// all it started: the group whose id it wrote, sent TERM, given five
    /// seconds, then KILL. `None` for `cmd.exe`, which has no such group.
    pub(crate) fn stop(self, dir: &str) -> Option<String> {
        match self {
            Self::Posix => Some(format!(
                "cd {} && p=$(cat {RUN_PID} 2>/dev/null) && test -n \"$p\" && {{ \
                 kill -TERM -\"$p\" 2>/dev/null; i=0; \
                 while kill -0 -\"$p\" 2>/dev/null && [ $i -lt 50 ]; do sleep 0.1; i=$((i+1)); done; \
                 kill -KILL -\"$p\" 2>/dev/null; rm -f {RUN_PID}; }}; true",
                self.word(dir).expect("any text")
            )),
            Self::Cmd => None,
        }
    }

    /// The line as ssh is given it. A POSIX host's goes to `sh`, whole: the
    /// account's login shell may be one a POSIX line means nothing to.
    /// `cmd.exe` is given its line as it is.
    pub(crate) fn over_ssh(self, script: &str) -> String {
        match self {
            Self::Posix => format!("sh -c {}", Self::Posix.word(script).expect("any text")),
            Self::Cmd => script.to_string(),
        }
    }

    /// The program a run could not find, when that is how it ended: read
    /// from what the shell said of it among the last `lines` it printed,
    /// since the command's first word may be `cd`, a variable set for it, or
    /// a tool that ran another; and, said in no words known here, the first
    /// word of `command`. `None` for a run that ended any other way.
    pub(crate) fn not_found(
        self,
        exit: Option<i32>,
        lines: &[String],
        command: &str,
    ) -> Option<String> {
        let named = match self {
            Self::Posix if exit == Some(POSIX_NOT_FOUND) => lines.iter().rev().find_map(|line| {
                // `sh: 1: x: not found`, `bash: line 1: x: command not found`
                // and `zsh:1: command not found: x`.
                if let Some((_, name)) = line.rsplit_once("command not found: ") {
                    return Some(name.trim().to_string());
                }
                let before = line
                    .strip_suffix(": command not found")
                    .or_else(|| line.strip_suffix(": not found"))?;
                Some(before.rsplit(": ").next()?.trim().to_string())
            }),
            // `'x' is not recognized as an internal or external command,`
            // then the line the sentence ends on.
            Self::Cmd if exit == Some(1) && lines.iter().any(|l| l.contains(CMD_NOT_FOUND)) => {
                lines.iter().find_map(|line| {
                    let rest = line.trim().strip_prefix('\'')?;
                    let (name, said) = rest.split_once('\'')?;
                    said.contains("is not recognized").then(|| name.to_string())
                })
            }
            Self::Posix | Self::Cmd => return None,
        };
        let first = || {
            command
                .split_whitespace()
                .next()
                .unwrap_or(command)
                .trim_matches(['\'', '"'])
                .to_string()
        };
        Some(named.filter(|name| !name.is_empty()).unwrap_or_else(first))
    }

    /// Several words of a command line as one line: each a word of its own
    /// for a POSIX shell; for `cmd.exe`, quoted only when it holds anything
    /// but the letters a bare word is made of.
    ///
    /// A word `cmd.exe` is given in quotes cannot hold one, and one ending in
    /// a backslash has it doubled: the program it is handed to would read
    /// `\"` as a quote of its own.
    pub(crate) fn join(self, words: &[String]) -> Result<String, String> {
        let bare = |word: &str| {
            !word.is_empty()
                && word
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "_./\\:=-+,@".contains(c))
        };
        let joined = words
            .iter()
            .map(|word| match self {
                Self::Posix => Self::Posix.word(word),
                Self::Cmd if bare(word) => Ok(word.clone()),
                Self::Cmd if word.contains('"') => Err(format!(
                    "{word} holds a quote; give the whole command as one word after --"
                )),
                Self::Cmd if word.ends_with('\\') => Ok(format!("\"{word}\\\"")),
                Self::Cmd => Ok(format!("\"{word}\"")),
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(joined.join(" "))
    }
}

/// `command` with every character the `cmd.exe` that reads the whole line
/// would act on escaped, so it passes it on untouched: its quotes, which
/// would otherwise switch that shell's own quoting on and off, and what it
/// reads as an operator. `%` is left: a variable is the person's to write.
fn for_outer_cmd(command: &str) -> String {
    let mut escaped = String::with_capacity(command.len() + 8);
    for c in command.chars() {
        if "^&|<>()\"".contains(c) {
            escaped.push('^');
        }
        escaped.push(c);
    }
    escaped
}
