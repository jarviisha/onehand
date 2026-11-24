//! A command of the project's that its agents' builds run on a host: the
//! words that name it, and what comes back.

use std::path::PathBuf;

use super::inside_refusal;

/// A command run on the project's build host in place of here.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RoutedCommand {
    /// The program and the leading words after it, such as `cargo build`.
    pub words: String,
    /// What the command brings back, as paths inside the project.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<PathBuf>,
}

impl RoutedCommand {
    /// From a form's two fields: `artifacts` one path a line, blank lines
    /// dropped. The program is a plain name, never a path, so what runs is
    /// what the host's own search finds.
    pub fn parse(words: &str, artifacts: &str) -> Result<Self, String> {
        let words = words.split_whitespace().collect::<Vec<_>>().join(" ");
        let program = words.split(' ').next().unwrap_or("");
        if program.is_empty() {
            return Err("Name the command".into());
        }
        if !plain_program(program) {
            return Err(format!("{program} must be a program's name, not a path"));
        }
        let artifacts = artifacts
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(|path| match inside_refusal(path) {
                Some(why) => Err(why),
                None => Ok(PathBuf::from(path)),
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self { words, artifacts })
    }

    /// The artifacts as the form's field holds them, one path a line:
    /// what `parse` reads back as these.
    pub fn artifacts_lines(&self) -> String {
        self.artifacts
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The commands a folder holding `top_names` at its top suggests
    /// routing: a Rust workspace's and a .NET solution's builds.
    pub fn suggested(top_names: &[String]) -> Vec<&'static str> {
        let cargo = top_names.iter().any(|name| name == "Cargo.toml");
        let dotnet = top_names
            .iter()
            .any(|name| name.ends_with(".sln") || name.ends_with(".csproj"));
        let mut found = Vec::new();
        if cargo {
            found.extend(["cargo build", "cargo test", "cargo clippy"]);
        }
        if dotnet {
            found.extend(["dotnet build", "dotnet test"]);
        }
        found
    }

    /// The program's name.
    pub fn program(&self) -> &str {
        self.words.split(' ').next().unwrap_or("")
    }

    /// Whether `program args` is this command: the same program, and after
    /// any leading options (`-q`, `+nightly`) the same next words.
    pub(crate) fn matches(&self, program: &str, args: &[String]) -> bool {
        if self.program() != program {
            return false;
        }
        let mut rest = args.iter().skip_while(|a| a.starts_with(['-', '+']));
        self.words
            .split(' ')
            .skip(1)
            .all(|word| rest.next().is_some_and(|a| a == word))
    }
}

/// Whether `program` is a plain program name: ASCII letters, digits, `.`,
/// `_` and `-`, starting with neither `.` nor `-`. Nothing a shell reads as
/// more than one word, nor a path, nor an option.
pub(crate) fn plain_program(program: &str) -> bool {
    !program.is_empty()
        && !program.starts_with(['.', '-'])
        && program
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

#[cfg(test)]
mod tests;
