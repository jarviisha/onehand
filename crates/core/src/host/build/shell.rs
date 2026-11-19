//! The words each step of a build says to a host, in the host's own shell.
//!
//! ssh hands a line to the account's shell: a POSIX one on Linux, `cmd.exe`
//! on Windows. The steps of a build are the same on both; only how a word is
//! quoted and how each step is spelled differ, and that is all this holds.

use std::path::{Path, PathBuf};

use super::MARK;
use crate::config::HostOs;

/// The shell a host's ssh hands a line to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Shell {
    Posix,
    /// Windows' `cmd.exe`, the shell its OpenSSH starts unless changed.
    Cmd,
}

/// The exit code a POSIX shell gives a command it could not find.
const POSIX_NOT_FOUND: i32 = 127;

/// How `cmd.exe` ends what it says of a program it could not find. Its exit
/// code is 1, as any failure's, so its words are all there is to go by; they
/// are English on an English Windows only.
pub(super) const CMD_NOT_FOUND: &str = "operable program or batch file";

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

    /// Make the folder `dir`, and the repository in it. On Windows the
    /// repository is told to leave line endings alone and to take long
    /// paths, so the code is built there as it is here.
    pub(crate) fn init(self, dir: &str) -> Result<String, String> {
        let dir = self.word(dir)?;
        Ok(match self {
            Self::Posix => format!("mkdir -p {dir} && git -C {dir} init -q"),
            Self::Cmd => format!(
                "git init -q {dir} && git -C {dir} config core.autocrlf false && \
                 git -C {dir} config core.longpaths true"
            ),
        })
    }

    /// In `dir`, check the snapshot `sha` out, go to `within`, print the
    /// mark, and run `command` with what it says on stderr among its lines.
    ///
    /// The command is the person's own line for that shell: run by the login
    /// shell on a POSIX host, so a toolchain a profile puts on `PATH` is
    /// found, and by a `cmd.exe` of its own on Windows, so the whole line's
    /// stderr is folded in, not its last command's.
    ///
    /// **On Windows the line is read twice**, by the `cmd.exe` ssh starts and
    /// by the one that runs the command, and quotes there toggle rather than
    /// nest: a `|` inside the command's own quotes would be a pipe to the
    /// first. So every character the first would act on is escaped
    /// ([`for_outer_cmd`]), and the second is handed the line as written.
    pub(crate) fn run(
        self,
        dir: &str,
        sha: &str,
        within: &Path,
        command: &str,
    ) -> Result<String, String> {
        let dir = self.word(dir)?;
        let within = match within.as_os_str().is_empty() {
            true => String::new(),
            false => format!("cd {} && ", self.word(&within.display().to_string())?),
        };
        Ok(match self {
            Self::Posix => format!(
                "cd {dir} && git checkout -q -f --detach {sha} && {within}echo {MARK} && \
                 exec \"${{SHELL:-/bin/sh}}\" -lc {} 2>&1",
                self.word(command)?
            ),
            Self::Cmd => format!(
                "cd /d {dir} && git checkout -q -f --detach {sha} && {within}echo {MARK} && \
                 cmd /d /s /c ^\"{}^\" 2>&1",
                for_outer_cmd(command)
            ),
        })
    }

    /// In `dir`, send `paths` as a tar on stdout, or exit 3 naming on stderr
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
                 exit 3; }}; done && tar -cf - -- {paths}"
            ),
            Self::Cmd => format!(
                "cd /d {dir} && (for %p in ({paths}) do @if not exist %p (echo %~p>&2 & exit 3)) \
                 && tar -cf - -- {paths}"
            ),
        })
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

    /// Whether a run that ended with `exit`, the last line it printed being
    /// `last`, could not find its program.
    pub(crate) fn not_found(self, exit: Option<i32>, last: &str) -> bool {
        match self {
            Self::Posix => exit == Some(POSIX_NOT_FOUND),
            Self::Cmd => exit == Some(1) && last.contains(CMD_NOT_FOUND),
        }
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
