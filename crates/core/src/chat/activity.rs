//! Semantic presentation for transcript activity rows.
//!
//! ACP gives us broad tool kinds, while an execute title is usually a raw shell
//! command.  This module turns both into calm, human-readable actions for the
//! collapsed transcript; the exact command remains available in the expanded
//! detail surface.

use crate::acp::{ToolContent, ToolKind};
use crate::chat::model::{ChatItem, ToolItem};

/// The first line of `s`, trimmed and clipped to `max` characters.
///
/// A tool title comes straight from the agent -- a heredoc Bash script arrives
/// as one unbounded "line" -- so every descriptor is clipped before it reaches
/// a renderer.
pub fn first_line_trunc(s: &str, max: usize) -> String {
    let line = s.lines().next().unwrap_or("").trim();
    if line.chars().count() > max {
        let head: String = line.chars().take(max).collect();
        format!("{head}…")
    } else {
        line.to_string()
    }
}

/// Coarse transcript sections. Tool rows keep their precise semantic action,
/// while adjacent rows with the same section read as one scan-friendly block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActivityGroup {
    Explored,
    Changed,
    Ran,
    Verified,
    Reasoned,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActivityKind {
    Change,
    Check,
    Inspect,
    Search,
    Fetch,
    Test,
    Build,
    Run,
    Reason,
    Other,
}

impl From<ActivityKind> for ActivityGroup {
    fn from(kind: ActivityKind) -> Self {
        match kind {
            ActivityKind::Inspect | ActivityKind::Search | ActivityKind::Fetch => Self::Explored,
            ActivityKind::Change => Self::Changed,
            ActivityKind::Build | ActivityKind::Run => Self::Ran,
            ActivityKind::Check | ActivityKind::Test => Self::Verified,
            ActivityKind::Reason => Self::Reasoned,
            ActivityKind::Other => Self::Other,
        }
    }
}

pub fn group(item: &ChatItem) -> Option<ActivityGroup> {
    match item {
        ChatItem::Thought(_) => Some(ActivityGroup::Reasoned),
        ChatItem::Tool(tool) => Some(presentation(tool).kind.into()),
        _ => None,
    }
}

pub struct Presentation {
    pub action: &'static str,
    pub subject: String,
    pub metadata: Option<String>,
    // Retained as part of the semantic presentation contract and covered by
    // classification tests, even though the calmer aggregate header no longer
    // prints per-kind counts.
    #[cfg_attr(not(test), allow(dead_code))]
    pub kind: ActivityKind,
}

pub fn presentation(tool: &ToolItem) -> Presentation {
    let call = &tool.call;
    match call.kind {
        ToolKind::Read => present(
            "Inspected",
            strip_redundant_prefix(&call.title, &["Read", "Inspect", "Inspected"]),
            ActivityKind::Inspect,
        ),
        ToolKind::Edit => {
            let created = call
                .content
                .iter()
                .any(|content| matches!(content, ToolContent::Diff { old: None, .. }));
            let path = first_diff_path(tool).unwrap_or(call.title.as_str());
            present(
                if created { "Created" } else { "Edited" },
                path,
                ActivityKind::Change,
            )
        }
        ToolKind::Delete => present("Deleted", &call.title, ActivityKind::Change),
        ToolKind::Move => present("Moved", &call.title, ActivityKind::Change),
        ToolKind::Search => present(
            "Searched",
            strip_redundant_prefix(&call.title, &["Search", "Searched", "Grep"]),
            ActivityKind::Search,
        ),
        ToolKind::Fetch => present(
            "Fetched",
            strip_redundant_prefix(&call.title, &["Fetch", "Fetched"]),
            ActivityKind::Fetch,
        ),
        ToolKind::Think => present("Reasoned", &call.title, ActivityKind::Reason),
        ToolKind::Other => present("Used tool", &call.title, ActivityKind::Other),
        ToolKind::Execute => execute_presentation(&call.title),
    }
}

fn present(action: &'static str, subject: &str, kind: ActivityKind) -> Presentation {
    Presentation {
        action,
        subject: first_line_trunc(subject, 240),
        metadata: None,
        kind,
    }
}

fn strip_redundant_prefix<'a>(title: &'a str, prefixes: &[&str]) -> &'a str {
    let title = title.trim();
    prefixes
        .iter()
        .find_map(|prefix| {
            let head = title.get(..prefix.len())?;
            let rest = title.get(prefix.len()..)?;
            (head.eq_ignore_ascii_case(prefix)
                && rest.chars().next().is_some_and(char::is_whitespace))
            .then(|| rest.trim_start())
        })
        .unwrap_or(title)
}

fn first_diff_path(tool: &ToolItem) -> Option<&str> {
    tool.call.content.iter().find_map(|content| match content {
        ToolContent::Diff { path, .. } if !path.trim().is_empty() => Some(path.as_str()),
        _ => None,
    })
}

fn execute_presentation(command: &str) -> Presentation {
    let semantic_line = semantic_command(command);
    let line = semantic_line.trim();
    let tokens: Vec<&str> = line.split_whitespace().collect();
    let Some((program_idx, program)) = primary_program(&tokens) else {
        return present("Ran", line, ActivityKind::Run);
    };
    let args = &tokens[program_idx + 1..];

    // Unwrap the common shell-launcher shape so `bash -lc 'rg …'` reads as a
    // search instead of an opaque Bash invocation.
    if matches!(
        program,
        "bash" | "sh" | "zsh" | "fish" | "pwsh" | "powershell"
    ) && args
        .iter()
        .any(|arg| arg.contains('c') || *arg == "-Command")
    {
        if let Some(inner) = quoted_argument(line) {
            if inner.trim() != line {
                return execute_presentation(&inner);
            }
        }
    }

    if is_test_command(program, args) {
        let label = command_label(program, args, &["nextest", "test"]);
        return present("Ran tests", &label, ActivityKind::Test);
    }
    if is_check_command(program, args) {
        let label = command_label(program, args, &["clippy", "check", "lint", "fmt"]);
        return present("Checked", &label, ActivityKind::Check);
    }
    if is_build_command(program, args) {
        let label = command_label(program, args, &["build", "compile"]);
        return present("Built", &label, ActivityKind::Build);
    }
    if matches!(program, "grep" | "rg" | "ripgrep" | "ag" | "ack") {
        let (subject, metadata) = search_parts(line, args);
        return Presentation {
            action: "Searched",
            subject,
            metadata,
            kind: ActivityKind::Search,
        };
    }
    if is_inspect_command(program, args) {
        let subject = inspect_target(program, args);
        return present("Inspected", &subject, ActivityKind::Inspect);
    }
    if matches!(program, "curl" | "wget" | "http" | "xh") {
        let subject = url_argument(args)
            .or_else(|| last_argument(args))
            .unwrap_or_else(|| program.to_string());
        return present("Fetched", &subject, ActivityKind::Fetch);
    }
    if matches!(program, "rm" | "rmdir") {
        let subject = last_argument(args).unwrap_or_else(|| "files".to_string());
        return present("Deleted", &subject, ActivityKind::Change);
    }
    if matches!(
        program,
        "mkdir" | "touch" | "cp" | "mv" | "install" | "apply_patch"
    ) {
        let subject = last_argument(args).unwrap_or_else(|| program.to_string());
        return present("Changed files", &subject, ActivityKind::Change);
    }

    present(
        "Ran",
        strip_redundant_prefix(line, &["Run", "Ran", "Execute", "Executed"]),
        ActivityKind::Run,
    )
}

/// Select the meaningful operation from a compound shell command. Setup and
/// plumbing (`cd`, captured variables, `echo`) should not become the activity
/// label when a later segment performs the actual network/test/search work.
fn semantic_command(command: &str) -> String {
    let flat = command
        .lines()
        .take_while(|line| line.trim() != "EOF")
        .collect::<Vec<_>>()
        .join(" ");
    let segments: Vec<&str> = flat
        .split(" && ")
        .flat_map(|part| part.split(" || "))
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect();

    for segment in &segments {
        let first = segment.split_whitespace().next().unwrap_or("");
        if first.contains("=$(") || first.contains("=`") {
            continue;
        }
        let tokens: Vec<&str> = segment.split_whitespace().collect();
        let Some((_, program)) = primary_program(&tokens) else {
            continue;
        };
        if matches!(
            program,
            "cd" | "pushd" | "popd" | "export" | "echo" | "printf"
        ) {
            continue;
        }
        return (*segment).to_string();
    }

    segments
        .first()
        .copied()
        .unwrap_or(command.lines().next().unwrap_or(""))
        .to_string()
}

fn primary_program<'a>(tokens: &'a [&'a str]) -> Option<(usize, &'a str)> {
    let mut skip_env_options = false;
    let mut skip_timeout_options = false;
    for (idx, raw) in tokens.iter().enumerate() {
        let token = clean_token(raw);
        if token.is_empty() || matches!(token, "sudo" | "command" | "builtin") {
            continue;
        }
        if token == "env" {
            skip_env_options = true;
            continue;
        }
        if token == "timeout" {
            skip_timeout_options = true;
            continue;
        }
        if skip_timeout_options
            && (token.starts_with('-')
                || token
                    .trim_end_matches(|c: char| c.is_ascii_alphabetic())
                    .parse::<f64>()
                    .is_ok())
        {
            continue;
        }
        if skip_env_options && (token.starts_with('-') || token.contains('=')) {
            continue;
        }
        if token.contains('=') && !token.contains('/') {
            continue;
        }
        if token.starts_with('-') {
            continue;
        }
        return Some((idx, token.rsplit('/').next().unwrap_or(token)));
    }
    None
}

fn clean_token(token: &str) -> &str {
    token.trim_matches(|c: char| matches!(c, ';' | '|' | '&' | '(' | ')' | '"' | '\'' | '`' | ','))
}

fn arg_is(args: &[&str], value: &str) -> bool {
    args.iter()
        .any(|arg| clean_token(arg).eq_ignore_ascii_case(value))
}

fn is_test_command(program: &str, args: &[&str]) -> bool {
    matches!(
        program,
        "pytest" | "py.test" | "vitest" | "jest" | "nextest"
    ) || (matches!(program, "cargo" | "go" | "dotnet" | "make") && arg_is(args, "test"))
        || (program == "cargo" && arg_is(args, "nextest"))
        || (matches!(program, "npm" | "pnpm" | "yarn" | "bun") && arg_is(args, "test"))
}

fn is_check_command(program: &str, args: &[&str]) -> bool {
    matches!(
        program,
        "eslint" | "ruff" | "tsc" | "clippy" | "rustfmt" | "shellcheck"
    ) || (program == "cargo"
        && (arg_is(args, "check") || arg_is(args, "clippy") || arg_is(args, "fmt")))
        || (matches!(program, "npm" | "pnpm" | "yarn" | "bun") && arg_is(args, "lint"))
        || (program == "biome" && arg_is(args, "check"))
}

fn is_build_command(program: &str, args: &[&str]) -> bool {
    (matches!(program, "cargo" | "go" | "dotnet" | "make") && arg_is(args, "build"))
        || (matches!(program, "npm" | "pnpm" | "yarn" | "bun") && arg_is(args, "build"))
}

fn is_inspect_command(program: &str, args: &[&str]) -> bool {
    matches!(
        program,
        "cat"
            | "head"
            | "tail"
            | "sed"
            | "awk"
            | "ls"
            | "find"
            | "fd"
            | "tree"
            | "stat"
            | "file"
            | "wc"
            | "pwd"
            | "which"
            | "whereis"
    ) || (program == "git"
        && args
            .first()
            .is_some_and(|arg| matches!(clean_token(arg), "status" | "diff" | "log" | "show")))
}

fn command_label(program: &str, args: &[&str], subcommands: &[&str]) -> String {
    args.iter()
        .map(|arg| clean_token(arg).to_ascii_lowercase())
        .find(|arg| subcommands.contains(&arg.as_str()))
        .map_or_else(|| program.to_string(), |sub| format!("{program} {sub}"))
}

fn search_parts(line: &str, args: &[&str]) -> (String, Option<String>) {
    if let Some((value, end)) = quoted_argument_span(line) {
        let metadata =
            first_argument(&line[end..]).map(|path| format!("in {}", first_line_trunc(&path, 120)));
        return (first_line_trunc(&value, 180), metadata);
    }

    let mut values = args
        .iter()
        .map(|arg| clean_token(arg))
        .filter(|arg| !arg.is_empty() && !arg.starts_with('-'));
    let subject = values
        .next()
        .map(|value| first_line_trunc(value, 180))
        .unwrap_or_else(|| "workspace".to_string());
    let metadata = values
        .next()
        .map(|path| format!("in {}", first_line_trunc(path, 120)));
    (subject, metadata)
}

fn inspect_target(program: &str, args: &[&str]) -> String {
    if program == "git" {
        return "repository".to_string();
    }
    last_argument(args).unwrap_or_else(|| {
        if program == "pwd" {
            "working directory".to_string()
        } else {
            program.to_string()
        }
    })
}

fn last_argument(args: &[&str]) -> Option<String> {
    args.iter()
        .take_while(|arg| !matches!(*arg, &"|" | &"&&" | &"||" | &";"))
        .map(|arg| clean_token(arg))
        .filter(|arg| {
            !arg.is_empty()
                && !arg.starts_with('-')
                && !arg.starts_with('>')
                && !arg
                    .chars()
                    .all(|c| c.is_ascii_digit() || matches!(c, ',' | 'p'))
        })
        .last()
        .map(|arg| first_line_trunc(arg, 56))
}

fn url_argument(args: &[&str]) -> Option<String> {
    args.iter()
        .map(|arg| clean_token(arg))
        .find(|arg| arg.starts_with("https://") || arg.starts_with("http://"))
        .map(|url| first_line_trunc(url, 56))
}

fn first_argument(suffix: &str) -> Option<String> {
    suffix
        .split_whitespace()
        .take_while(|arg| !matches!(*arg, "|" | "&&" | "||" | ";"))
        .map(clean_token)
        .find(|arg| !arg.is_empty() && !arg.starts_with('-') && !arg.starts_with('>'))
        .map(str::to_string)
}

fn quoted_argument(line: &str) -> Option<String> {
    quoted_argument_span(line).map(|(value, _)| value)
}

fn quoted_argument_span(line: &str) -> Option<(String, usize)> {
    for quote in ['"', '\''] {
        let Some(start) = line.find(quote) else {
            continue;
        };
        let rest = &line[start + quote.len_utf8()..];
        if let Some(end) = rest.find(quote) {
            let value = &rest[..end];
            if !value.trim().is_empty() {
                return Some((
                    value.to_string(),
                    start + quote.len_utf8() + end + quote.len_utf8(),
                ));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{execute_presentation, strip_redundant_prefix, ActivityKind};

    #[test]
    fn native_tool_titles_do_not_repeat_the_action() {
        assert_eq!(
            strip_redundant_prefix("Read internal/compute/repository.go", &["Read"]),
            "internal/compute/repository.go"
        );
        assert_eq!(
            strip_redundant_prefix("Search func BuildInteractionSequences", &["Search"]),
            "func BuildInteractionSequences"
        );
    }

    #[test]
    fn grep_becomes_a_search_with_only_its_pattern() {
        let row = execute_presentation(
            "grep -rn \"DbSet<Counter>|DbSet<Branch>\" BackEndAPI/Data/AppDbContext.cs",
        );
        assert_eq!(row.kind, ActivityKind::Search);
        assert_eq!(row.action, "Searched");
        assert_eq!(row.subject, "DbSet<Counter>|DbSet<Branch>");
        assert_eq!(
            row.metadata.as_deref(),
            Some("in BackEndAPI/Data/AppDbContext.cs")
        );
    }

    #[test]
    fn test_commands_get_a_task_level_summary() {
        let row = execute_presentation("cargo test --workspace --all-targets");
        assert_eq!(row.kind, ActivityKind::Test);
        assert_eq!(row.action, "Ran tests");
        assert_eq!(row.subject, "cargo test");
    }

    #[test]
    fn read_only_shell_commands_are_inspections() {
        let row = execute_presentation("sed -n '1,160p' src/chat/view.rs");
        assert_eq!(row.kind, ActivityKind::Inspect);
        assert_eq!(row.action, "Inspected");
        assert_eq!(row.subject, "src/chat/view.rs");
    }

    #[test]
    fn generic_commands_keep_their_arguments_in_the_single_line_summary() {
        let row = execute_presentation("go run ./cmd/server --debug");
        assert_eq!(row.action, "Ran");
        assert_eq!(row.subject, "go run ./cmd/server --debug");
    }

    #[test]
    fn generic_run_titles_do_not_repeat_the_action() {
        let row = execute_presentation("Run counter");
        assert_eq!(row.action, "Ran");
        assert_eq!(row.subject, "counter");
    }

    #[test]
    fn validation_build_and_wrapped_commands_are_classified() {
        let lint = execute_presentation("env RUSTFLAGS=-Dwarnings cargo clippy --workspace");
        assert_eq!(lint.kind, ActivityKind::Check);
        assert_eq!(lint.action, "Checked");
        assert_eq!(lint.subject, "cargo clippy");

        let build = execute_presentation("pnpm run build");
        assert_eq!(build.kind, ActivityKind::Build);
        assert_eq!(build.action, "Built");

        let wrapped = execute_presentation("bash -lc 'rg -n TODO src'");
        assert_eq!(wrapped.action, "Searched");
        assert_eq!(wrapped.subject, "TODO");
        assert_eq!(wrapped.metadata.as_deref(), Some("in src"));
    }

    #[test]
    fn compound_shell_setup_does_not_become_the_activity_label() {
        let login = execute_presentation(
            "cd /tmp && timeout 60 curl -sS -X POST 'https://example.test/api/login' -o out.json && head out.json",
        );
        assert_eq!(login.action, "Fetched");
        assert_eq!(login.subject, "https://example.test/api/login");

        let request = execute_presentation(
            "TOKEN=$(python3 -c \"print('x')\") && echo \"$TOKEN\" > token.txt && timeout 80 curl -sS https://example.test/api/banks",
        );
        assert_eq!(request.action, "Fetched");
        assert_eq!(request.subject, "https://example.test/api/banks");
    }

    #[test]
    fn activity_copy_has_a_golden_regression_snapshot() {
        let commands = [
            "grep -rn \"DbSet<Counter>\" BackEndAPI/Data/AppDbContext.cs",
            "sed -n '1,160p' src/chat/view.rs",
            "cargo test --workspace",
            "cargo clippy --workspace",
            "cargo build --release",
        ];
        let actual = commands
            .iter()
            .map(|command| {
                let row = execute_presentation(command);
                format!(
                    "{} | {} | {} | {:?}",
                    row.action,
                    row.subject,
                    row.metadata.as_deref().unwrap_or("-"),
                    row.kind
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(
            actual,
            include_str!("snapshots/activity_rows.txt").trim_end()
        );
    }
}
