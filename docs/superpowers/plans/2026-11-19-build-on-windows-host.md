# Build On A Windows Host Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A project builds on a declared Windows host as it does on a Linux one: the same flag,
tool and page, the same five steps.

**Architecture:** What differs between the two systems is only the words each step says to the
host's shell, and how the push reaches it. Those words move out of `host/build.rs` into
`host/build/shell.rs` as one enum, `Shell { Posix, Cmd }`, picked from the host's declared system;
the steps themselves do not change.

**Tech Stack:** Rust, `git`, `ssh`, `tar`; Windows' own `cmd.exe`, `powershell.exe` and `tar.exe`.

**Spec:** `docs/superpowers/specs/2026-11-19-hosts-and-builds-design.md` (this plan is its PR 3).
Builds on PR #365 (`feat/build-on-host`); this branch is cut from it.

## Global Constraints

- Everything PR 2's plan held to still holds: core GUI-free and blocking, rules and their words
  in core, exhaustive matches, no ignored file travels, artifacts only on exit 0 and only the
  paths named unpacked, one build per folder and host.
- A Windows host is one declared with `os = "windows"`; an undeclared host stays Linux.
- A Windows host is reached with OpenSSH's default shell, `cmd.exe`. One whose default shell was
  changed cannot be probed and does not build; that gap is already written.
- Checked by hand on Windows Server 2019 (OpenSSH, `cmd.exe`, Git for Windows, `tar.exe`):
  - `git init -q "<dir>"` makes the folder, relative to the home ssh starts in.
  - `git push --receive-pack="powershell -NoProfile git receive-pack" <host>:<dir>` reaches a
    home-relative folder; a plain push fails on `cmd.exe`'s quoting.
  - `cd /d "<dir>" && git checkout -q -f --detach <sha> && cd "<within>" && echo <mark> &&
    <command> 2>&1` runs, with forward slashes in `<within>`, and its exit code comes back.
  - A program that is not there exits 1, saying `'x' is not recognized as an internal or
    external command`; no exit code tells it apart.
  - `(for %p in ("a" "b") do @if not exist %p (echo %~p>&2 & exit 3)) && tar -cf - -- "a" "b"`
    sends the paths, or exits 3 naming the missing one, with a trailing space.
- No comment or string names a document. Every `.md` is English. The last commit removes this
  plan and, this being the last of the three PRs, the spec.

**Choices this plan makes, for the owner to confirm:**

1. The command is a `cmd.exe` line, run by `cmd.exe`: `dotnet build`, `cargo build && dir`. It is
   not run through PowerShell.
2. A program the host lacks is recognised by `cmd.exe`'s own English sentence, since its exit
   code is 1 like any failure. On a Windows that speaks another language the build still fails
   plainly; only the *is not on <host>'s PATH for ssh* line is missing.
3. A folder, an artifact or an `into` holding `%` or `"` is refused for a Windows host: `cmd.exe`
   expands `%` even inside quotes, and no file name there can hold `"`.
4. The repository the host keeps is set `core.autocrlf false` and `core.longpaths true`, so the
   code is built as it is here, line endings included.
5. Several words after `--` are joined for `cmd.exe`: a word with a space or a shell character is
   put in double quotes, the others left bare.

## Review Focus

1. A folder or artifact with a space on a Windows host: quoted, and found. Test in Task 1.
2. A `%` or a `"` in any word onehand writes for `cmd.exe`: refused before anything is sent.
   Test in Task 1.
3. Every PR 2 behaviour on a Linux host and on `local`: unchanged. The whole existing suite in
   Task 1.
4. A Windows host declared but with no `git`: the build's own failure names the host. No test;
   the probe's row already says it.
5. A Windows build whose output is CRLF: each line handed over without its `\r`. Test in Task 1.

---

### Task 1: The words each shell is told

**Files:**
- Create: `crates/core/src/host/build/shell.rs`
- Modify: `crates/core/src/host/build.rs` (uses `Shell`; `Job` gains the system),
  `crates/core/src/host/build/tests.rs`

**Interfaces:**
- Produces:

```rust
/// The shell a host's ssh hands a line to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Shell { Posix, Cmd }

impl Shell {
    pub(crate) fn of(os: &HostOs) -> Self;                 // Windows is Cmd; every other, Posix
    /// `text` as one word, or why it cannot be one there.
    pub(crate) fn word(self, text: &str) -> Result<String, String>;
    /// Make the folder and the repository in it.
    pub(crate) fn init(self, dir: &str) -> Result<String, String>;
    /// Check `sha` out, go to `within`, print the mark, run `command`.
    pub(crate) fn run(self, dir: &str, sha: &str, within: &Path, command: &str) -> Result<String, String>;
    /// Send `paths`, or exit 3 naming the first that is not there.
    pub(crate) fn send(self, dir: &str, paths: &[PathBuf]) -> Result<String, String>;
    /// The line as ssh is given it: through `sh -c` for Posix, as it is for Cmd.
    pub(crate) fn over_ssh(self, script: &str) -> String;
    /// Whether a run that ended `exit`, its last line `last`, could not find its program.
    pub(crate) fn not_found(self, exit: Option<i32>, last: &str) -> bool;
    /// Several words of a command line as one line for this shell.
    pub(crate) fn join(self, words: &[String]) -> String;
}

pub struct Job<'a> { /* … */ pub os: HostOs }
```

`Cmd`'s refusal: *<text> cannot be built on a Windows host: its name holds % or "*. `Posix`'s
scripts are PR 2's, moved unchanged. The push for `Cmd` adds
`--receive-pack=powershell -NoProfile git receive-pack` and names the folder `<host>:<dir>`;
`init` for `Cmd` is `git init -q "<dir>" && git -C "<dir>" config core.autocrlf false && git -C
"<dir>" config core.longpaths true`.

- [ ] **Step 1: Failing tests** on the strings and rules: `cmd_words_are_double_quoted`;
  `cmd_refuses_a_percent_or_a_quote`; `cmd_run_is_the_line_checked_by_hand` (the exact line
  above for a folder, a sha, `sub dir` and `dotnet build`); `cmd_send_checks_then_tars`;
  `cmd_goes_over_ssh_as_it_is_and_posix_through_sh`; `cmd_not_found_is_read_from_its_sentence`
  (exit 1 with the sentence is, exit 1 with another line is not, exit 127 is for Posix only);
  `cmd_join_quotes_only_what_needs_it` (`["dotnet","build","my app.sln"]` →
  `dotnet build "my app.sln"`); `posix_scripts_are_what_they_were` (the PR 2 strings).
- [ ] **Step 2:** `cargo test -p onehand-core host::build` — fails to compile.
- [ ] **Step 3:** Implement; `build_in` asks `Shell::of(&job.os)` for every line and for the push.
- [ ] **Step 4:** `cargo test -p onehand-core` — passes, every PR 2 test with it.
- [ ] **Step 5:** Commit `refactor(build): say each step in the words of the host's shell`.

### Task 2: A Windows host builds

**Files:**
- Modify: `crates/core/src/host/build/cli.rs` (`os_refusal` lets Windows through; `Asked` and
  `entry_for` carry the system; `command_line` takes the shell), `crates/app/src/control.rs` (the
  tool passes the system), `crates/app/src/settings/hosts.rs` and its tests (the row's note),
  `crates/core/src/host/build/tests.rs`

**Interfaces:**
- Consumes: `Shell`, `Job.os`.
- Produces: `entry_for(...) -> Result<(BuildHost, HostOs), String>`;
  `command_line(words: &[String], shell: Shell) -> Result<String, String>`; `Asked.os: HostOs`.

A Windows host's row on the Hosts page says *Builds only*, as a Linux one that runs no sessions
does; only a system this build does not know keeps *nothing builds on it yet*, and is still
refused, *<host> runs <os>; nothing builds on it yet*.

- [ ] **Step 1: Failing tests:** `a_windows_host_is_no_longer_refused`
  (`entry_for` answers the entry with `HostOs::Windows`); `a_system_this_build_does_not_know_is_still_refused`;
  `several_words_for_a_windows_host_are_joined_for_cmd`; the Hosts page's
  `a_windows_host_says_it_builds_only`.
- [ ] **Step 2:** run them — fail.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** `cargo test --locked` — passes.
- [ ] **Step 5:** Commit `feat(build): build a project on a Windows host`.

### Task 3: Tried on a real host, the documents, and the checks CI runs

**Files:**
- Modify: `docs/hosts.md` (*Builds on a host*: the two shells and what differs), `docs/screens.md`
  (the Hosts row's note), `docs/known-gaps.md` (drop *Nothing builds on a Windows host yet*; add:
  a Windows host is asked through `cmd.exe` only; a missing program is recognised in English
  only; `%` and `"` in a name), `CHANGELOG.md`
- Delete: this plan, and `docs/superpowers/specs/2026-11-19-hosts-and-builds-design.md`

- [ ] **Step 1:** With the built binary and a scratch config folder, build a scratch repository
  on the owner's Windows host: the cases PR 2 tried on Linux (first and second build, a subfolder
  with a space, artifacts with `into`, a failing command, a missing program, a missing artifact),
  then one real `dotnet new console` and `dotnet build` with its `bin` brought back. Remove what
  it left on the host.
- [ ] **Step 2:** Write the sections.
- [ ] **Step 3:** `make fmt`, `cargo test --locked`, `make clippy CLIPPY_EXTRA="-- -D warnings"`,
  `cargo tree -p onehand-core -i gpui-pre`.
- [ ] **Step 4:** Commit `docs(hosts): say how a project builds on a Windows host`.
