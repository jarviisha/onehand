# Routed builds: an agent's build runs on the project's build host

Status: design, 2026-11-23. Owner decisions are marked *(owner)*; calls made while writing it are
marked *(call)* and may be overturned.

## Why

Rust and .NET builds take the laptop's CPU and memory away from everything else. onehand can
already build a project on a host (`onehand --build <host> -- <command…>`, the tool
`build_on_host`, a project's build hosts with their artifacts, `docs/hosts.md#builds-on-a-host`).
But an agent does not use any of that unless it is told to, and the owner runs several agents
(Claude Code, Codex, Copilot), each read from its own instruction files. The host is also often
offline, so building there has to be switched off and on quickly.

What is wanted: **when an agent onehand started runs a build command of a project, the command
runs on the project's build host, without the agent knowing or being told**; and a switch in
onehand's UI decides whether it does.

## Goals

- An agent's `cargo build` (or any command the project names) in a project on this machine, or a
  worktree of it, runs on the build host the project picked, prints as it goes, exits with the
  host's code, and brings back the artifacts when that command is one that does. *(owner)*
- Works for every agent alike: nothing agent-specific, no instruction file. *(owner)*
- Two switches *(owner)*:
  - **Per project**: which of its build hosts it builds on, or *This machine*. Its worktrees
    follow it.
  - **One for the whole machine**: builds on hosts on or off. Off, every project builds here;
    each project's pick is kept for when it is on again.
- A host that does not answer does not stop work: the command runs here, saying why, and the
  project page says so too. *(call)*

## Non-goals

- The person's own terminals: the terminal dock, a terminal outside onehand, `make dev`. They
  build here; `onehand --build` stays the way to send one by hand. *(owner)*
- Check commands and workflow check steps: they can name `onehand --build` already. *(owner)*
- Sessions on a host and offloaded projects: their agents run on the host already.
- Installing anything on a host, or detecting toolchains (unchanged from builds on a host).
- Knowing any toolchain in core: onehand does not know what `cargo` is. *(owner)*

## Anti-goals

- An agent's command silently doing something different from what it would do here: a routed
  command prints one line saying where it runs (and why, when it falls back), on stderr.
- A switch that needs a session restarted to take effect: both switches are read on every
  command.
- A wrong build with no way back: the machine switch off restores today's behaviour exactly.

## Approaches considered

- **A. A folder of one-line shim scripts on the agent's `PATH`** *(owner picked)*. When onehand
  starts an agent it writes, under its config directory, one script per program the projects
  name (`cargo`, `dotnet`), each `exec <this onehand> --route <program> "$@"`, and puts that
  folder first on the agent's `PATH`.
  - **A1. `onehand --route` decides and builds by itself**, ssh included. Rejected: an agent's
    sandbox (Codex's default `workspace-write`, Claude Code's optional one) cuts the network and
    writes outside the workspace, so under it nothing could reach the host, take the build lock
    or write what the page shows.
  - **A2. `onehand --route` hands the command to the running onehand over its socket**
    *(owner picked)*. The running process decides and builds, outside any sandbox, and streams
    the lines back; `--route` prints them and exits with the code. The state (the host's
    standing, the waiting) lives in the process, in a drive, where the page reads it.
- B. Symlinks to the onehand binary named `cargo` etc., dispatched on `argv[0]`. No scripts, but a
  new dispatch in `main`, and the whole binary loaded for every `cargo` call. Not picked.
- C. Each agent's own hooks (Claude Code's `PreToolUse`, Codex's equivalent). Precise, but one
  kind per agent and none for a new one; against the why. Rejected, as is telling agents through
  `AGENTS.md`/`CLAUDE.local.md`.

## The design

### What a project declares

Beside its build hosts, in its project file and on Settings ▸ Projects:

- **Routed commands**: each the leading words of a command, `cargo build`, `cargo test`,
  `cargo clippy`, `dotnet build`, and whether it **brings artifacts back** *(owner)*. One that
  does not, `test`, `clippy`, returns only its output and exit code and is never failed for a
  missing artifact. *(owner)*
- **A routed command's artifacts are its own**, not its build host's *(call)*: paths inside the
  project, checked as build hosts' are (`inside_refusal`), and they land where the command would
  have written them here, never under an `into`, since the agent runs `./target/debug/x` next.
  Only those **this run wrote** come back (changed on the host since the run began); one it did
  not write is left alone, not a failure. So `cargo build` may name both
  `target/debug/onehand` and `target/release/onehand`, and core never reads `--release` or
  `--target`. What comes back writes over what is here, as now. Untouched, the local build
  cache still rebuilds on the next local `cargo run`: a known gap.
- A person's `--build` keeps its build host's `artifacts`/`into`, all or none, as now.
- The form offers a starting list from what the project folder holds: `Cargo.toml` offers
  `cargo build` (artifacts), `cargo test`, `cargo clippy`; a `*.sln`/`*.csproj` offers
  `dotnet build` (artifacts), `dotnet test`. Offered, never added by itself; the list written is
  the project's. *(owner: declared, with suggestions)* The suggestion table is the app's form
  help, not a rule core applies.
- **The project's pick**: one of its build hosts, or none for *This machine*. Kept in the project
  file. A project with no build host shows no pick.

### The machine switch

*Builds on hosts*, on or off, kept in the config directory's state (`state.toml`), not in
`onehand.toml` *(call: it is this laptop's state of the moment, not a setting)*. Default on.

### The socket's route call

`onehand --route <program> <args…>` is a thin client. It connects to the instance socket of the
config directory it was written for (`<config dir>/onehand.sock`, `docs/control.md`), sends one
first line, `onehand/route`, with the program, the arguments, the working folder and whether it
runs inside a run here (below), and prints what comes back as it comes: the command's lines to
stdout, onehand's own lines to stderr, then exits with the code in the final answer.

- **Nobody on the socket**, or the socket cannot be reached (onehand closed, a sandbox that
  forbids even a Unix socket): run here, after one stderr line *onehand is not reachable;
  building here*. *(call)*
- **The agent stops the command** (kills `--route`, its tool times out): the connection drops,
  and the running process stops the command on the host as a person's cancel of `--build` would.
- The call only runs a project's declared routed command, never a free command line: what is not
  routed is answered "run here", and `--route` execs the real program itself, inside the agent's
  own sandbox as before.
- **Security, accepted by the owner:** an agent whose sandbox has no network still runs code on
  the build host through a routed command (`build.rs`, tests), limited to the project's declared
  commands.

**Step 0 of the plan is a spike**: from a Codex session in its default sandbox, and from Claude
Code with its sandbox on, a shim reaches the socket and gets an answer. If a sandbox forbids the
Unix socket, the plan comes back to the owner before building further.

### When a command is routed

The running process decides, in this order:

1. The folder belongs to a project onehand keeps, its own folder or a worktree of it, found as
   `--build` finds it now (`asked_blocking`). Else: run here.
2. The machine switch is on, the project has a pick, and the pick is still one of its build
   hosts. Else: run here.
3. `<program> <args>` starts with one of the project's routed commands, word for word, after
   leading words that start with `-` or `+` (`cargo +nightly build`, `cargo -q build`) are
   skipped *(call)*. Else: run here.
4. The host runs the same system as this machine (Linux here routes to a Linux host only:
   another system compiles other `cfg` code, so errors and tests would differ). Else: run here.
   The pick's menu lists only such hosts. *(call)*
5. Build on the host, as `onehand --build <host> -- <program> <args…>` does today, with the
   command's own artifacts (below), and wait for a build already running in that folder on that host
   rather than refuse *(call; a person's `--build` still refuses)*, printing *waiting for the
   build already running on <host>* while it waits.
6. A host that cannot be reached (ssh fails to connect within its limit, before anything is
   sent) is a fall back, not a failure: run here, after one stderr line
   *<host> did not answer; building here*. Any other failure of a step is the command's failure,
   as `--build` reports it now. *(call)*

**Nested commands** are routed: a `make test` that calls `cargo test` sends the `cargo test`.
But a command that fell back to running here (host off, not reachable, not answering) runs with
`ONEHAND_ROUTED_HERE=1` in its environment, and `--route` under it runs here at once without
asking, so a slow or absent host is not tried again for every nested call. *(call)*

"Run here" execs the real program: the first `<program>` on `PATH` that is not in the shim
folder, with the same arguments, folder and environment. A program found nowhere exits 127 with
the shell's usual words.

Every routed run prints first, on stderr, *running on <host>* *(call)*, so the agent's transcript
shows where it ran.

### Where the shim folder goes on

- The agent's own process (`AgentSpec::launch`, run from `crates/io/src/acp.rs`), so its shell
  tool inherits it, and every terminal an agent asks for over ACP
  (`crates/core/src/acp/terminal.rs`). Nothing else: not the terminal dock, not check commands.
- Only for sessions whose agent runs on this machine.
- The folder holds a script for every program any project of this instance routes, and is
  rewritten when a routed command list changes. It belongs to the instance's own config root,
  and its scripts name that instance's own binary, so a `make dev` build and the installed one
  never route through each other.
- A session already open gets the folder when it starts; one opened before any project routed
  anything starts without it until it is opened again. Known and said in `docs/known-gaps.md`
  *(call)*.

### What is shown

- **The project page head**, beside *Run check*, for a project with build hosts: a chip
  *Builds: This machine* or *Builds: <host>*; its menu lists *This machine* and each build host,
  the pick ticked; below them the machine switch as a row, *Build on hosts* with its state.
  With the machine switch off the chip reads *Builds: here (hosts off)*.
- **A host that did not answer**: the chip reads *Builds: here (<host> not answering)* in the
  warning ink, from the host's standing in the drive, until a routed run reaches it again.
- **The command palette** offers *Turn builds on hosts off* / *on*, the quick way while a host is
  known to be down.
- **Settings ▸ Projects**: under *Build hosts*, *Routed commands*: one row each, its words and
  *brings artifacts back* or not, edit and remove, at most 20 *(call)*; *Add routed command…* a
  dialog with the words, the switch, and the suggestions.

### Where it lives

- Core: the routed-command model and parsing, the decision (steps 1–6) as a function of the
  folder, the project file, the switch, the host's standing and the args, the shim folder's
  contents, the `onehand/route` lines, the client side of `--route`. `onehand --route` dispatch
  beside `--build` in `main`, before the instance lock.
- A drive (a new one, or the build's place in the projects' drive; the builder's call) holds
  each host's standing as routed runs found it, and the queue of routed runs per folder and
  host; the page reads the standing from it. The socket serves the call on
  `onehand_io::socket`'s runtime and runs the build off the UI thread.
- A drive owns the changes: the project's pick and routed commands through `ProjectsDrive`
  (`put_build_host`'s neighbours); the machine switch through the drive that owns `state.toml`'s
  writes. The app's carriers only carry them out (`docs/drives.md`).
- Words and terms: `CONTEXT.md` gains **Routed command** (_Avoid_: intercepted, proxied,
  wrapped command); `docs/hosts.md` gains a *Routed builds* section; `docs/screens.md` the chip,
  its menu, the palette entry and the Settings rows.

## Constraints

- Core stays GUI-free and runtime-free; `onehand --route` runs before the instance lock, as
  `--build` does, and needs no window.
- The shim is POSIX `sh`: onehand's own machine is Linux or macOS. A Windows laptop is out of
  scope; build *hosts* stay Linux or Windows as now.
- An agent shell that resets `PATH` wholesale from a profile loses the shim. The plan verifies
  Claude Code's and Codex's shell tools keep a prepended `PATH`.
- Cost of a command that is not routed: one extra `exec` and one socket round trip; it must stay
  well under the time of the command itself.

## Left to the builder

- How "could not be reached" is told apart from other ssh failures (connect step exit code, a
  separate quick probe, or the push's own failure), as long as nothing is sent before deciding.
- The `onehand/route` messages' exact shape (one JSON-RPC notification per line out, a final
  answer with the code), within `docs/control.md`'s framing; the 64 KiB line limit applies.
- Exact shim file layout and when it is rewritten.
- PR split, if any (one feature, one PR with its docs is the house rule).
