# Declared hosts and builds on a host

Design, 2026-11-19. Not built. Where this says *decided*, the owner decided it in conversation;
*call* marks a choice made while writing, open to correction.

## Intent

The owner builds .NET and Rust all day, by hand and through session agents, and those builds take
the CPU and memory the laptop needs for other work. The builds should run on other machines, the
results should come back to the laptop, and onehand should know its hosts as things a person
declares and looks after, not only as a name typed on a project.

## Goals

- A build command runs on a host instead of the laptop, started by the person, by a session's
  agent, or by the check command, with its output and exit code coming back live.
- What the build produced comes back into the project folder on the laptop (decided).
- A host is Linux or Windows (decided). A Linux host can both run sessions, as today, and build
  (decided). A Windows host builds only (proposed, not objected to).
- One project can build on several hosts, for example Linux to run here and Windows to ship
  (decided).
- Hosts are declared once and managed from Settings (decided).

## Non-goals

- Sessions, agents, files, terminals or the host service on a Windows host.
- Several laptops attached to one host at once. Discussed and set aside; the one-laptop rule stays.
- Catching a bare `cargo build` an agent runs on the laptop. The agent is offered the tool and may
  still ignore it.
- A page listing builds, build history, or a shared build cache between hosts.
- macOS hosts, glob patterns in artifact paths, submodules.
- Installing a toolchain on a host, or running a setup command there for the person. onehand says
  what is missing and stops (decided).

## Anti-goals

- A build must never change the laptop's branch, index or working files other than the declared
  artifacts.
- A file git ignores must never travel to a host.
- A Windows host must never be offered where a session would be opened on it.

## Constraints

- Core stays GUI-free and dictates no runtime: the build is blocking functions in core, as
  `host/offload.rs` is. What happens next is a drive's; the app only carries it out.
- The existing mirror sync uses `rsync`, which Windows does not have, and the host service is
  Unix-only (Unix socket, lock, PTY, signals, `uname -sm`). A build host therefore uses neither.
- `onehand`'s first argument is a project folder unless it is a flag, so the command is a flag,
  never a subcommand.
- Glossary: the word is *host*; *VPS*, *server* and *remote* stay avoided. New terms proposed for
  `CONTEXT.md`: **build host** (a host a project builds on), **snapshot** (the commit a build
  carries to it), **artifact** (a path a build brings back).

## Approaches

How the source reaches the host:

1. **A git snapshot pushed over ssh, one path for both systems.** Picked by the owner. Only what
   changed travels, a file deleted here goes there too, ignored build folders stay on the host so
   builds are incremental. Costs `git` on the host and one Windows quirk (below).
2. **`rsync` for Linux and something else for Windows.** Reuses the offload code but keeps two
   sync paths, and Windows still needs 1 or 3.
3. **A `tar` of the whole tree each time.** Simplest and needs no `git` there, but resends
   everything every build and leaves deleted files behind on the host.

Running through the host service (an offloaded project, as built today) was also weighed and
dropped: it moves the agent as well, has no way to bring artifacts back, and cannot run on Windows
without porting the whole service.

### What the spike showed

Run against a Windows Server 2019 host (OpenSSH, default shell `cmd.exe`, Git for Windows, `tar`,
`dotnet`) with a scratch repository; nothing was left on the host.

- A snapshot built through a temporary index (`GIT_INDEX_FILE`, `git add -A`, `write-tree`,
  `commit-tree` on `HEAD`) carries tracked, changed and untracked files, leaves ignored ones, and
  leaves the real index and branch untouched.
- A plain `git push` to the Windows host fails: `cmd.exe` does not understand the single quotes
  git puts round the path. `git push --receive-pack="powershell -NoProfile git receive-pack"
  <host>:C:/path <sha>:refs/onehand/snap` works.
- `git checkout -f --detach <sha>` on the host removes what the new snapshot dropped and keeps the
  ignored `target/` from the build before.
- A command run as `cd /d <dir> && <command>` returns its output and its exit code through ssh.
- `ssh <host> "cd /d <dir> && tar -cf - <paths>" | tar -xf -` brings a file under an ignored
  folder back to the same relative path.

## Design

### Declared hosts

- `onehand.toml` keeps `[[hosts]]`, each with `name` (what ssh is given, as `HostName` is today)
  and what the probe found: `os` (`linux` or `windows`) and `arch` (as the host names it).
- **Which hosts run sessions:** Linux on x86_64 only, as today, because the host service is built
  for nothing else. Every other declared host, a Windows one or a Linux one on another
  architecture, builds only (call).
- **Settings ▸ Hosts** lists them: name, system, how it stands for this run (from `HostDrive` for
  a host that runs sessions), and the projects that use it, to run on or to build on.
  - *Add* takes a name, with the same picker of SSH aliases the host-project form has, probes the
    host over ssh and records its system. The probe also says whether `git` and `tar` are there,
    and a host missing either is added with that said on its row (call).
  - **The probe also names the toolchains it finds** (`cargo`, `rustc`, `dotnet`, each with its
    version) and the row lists them, so a person sees which host builds what before a build
    fails. They are told, never required: only `git` and `tar` block a build, refused before
    anything is sent, naming what is missing (decided).
  - *Check* probes again, after something was installed. *Remove* is refused while a project uses the host, naming the projects.
- A host a project already names but nobody declared is listed too, as Linux, and is written into
  `[[hosts]]` the first time Settings saves (call). Nothing a host project does today changes.
- The host-project form offers the declared hosts that run sessions first in its picker and never
  a build-only one.

### A project's build hosts

- Kept where the check command is kept, per project, in the workspace's config: a list of
  entries, each a `host`, its `artifacts` (paths inside the project, a file or a folder, no glob,
  not absolute, no `..`), and an optional `into` (a folder inside the project the artifacts land
  under). Without `into` they land on the same paths. `into` exists because two hosts building one
  project would otherwise write over each other's output (call).
- Edited in the project page's *Settings* box, beside *Check command*.
- Offered only for a project whose sessions run on this machine. A host project and an offloaded
  one already build on their host, and onehand's tools do not reach them (call).

### One build

`build(project, host, command)` in core, blocking, each step refusing in words that name the host:

1. **Snapshot** the folder as the spike did. The folder is the project's own or one of its
   worktrees: a worker's and a workflow's check command run in a worktree, and a snapshot needs
   no `.git` folder of its own, only git's objects (call). Refused when the folder is not a
   working tree's top, or holds another repository, which git would list as one folder.
2. **Push** it to the project's folder on the host, made and `git init`ed on first use:
   `~/.onehand-build/<laptop id>/<folder name>-<hash of its path>` (call; under `%USERPROFILE%`
   on Windows), the path being the working folder's, so each worktree keeps its own incremental
   state there. The laptop id is the one offload already keeps (`laptop`, private to
   `host/client/attach.rs` today).
3. **Check it out** there, detached and forced.
4. **Run** the command in that folder: through the login shell on Linux, so `~/.cargo/bin` is on
   PATH as it is for the host service; through `cmd.exe` on Windows. Output streams back line by
   line; the exit code is the build's.
   A command that fails because the host could not find its program ends with one more line,
   *<program> is not on <host>'s PATH for ssh* (decided), since a toolchain that is installed
   but not on the PATH an ssh command gets is the common case: on Windows the OpenSSH service
   must be restarted after an install before it sees the new PATH.
5. **Bring back** the artifacts, only when the command exited 0. An artifact that is not there
   fails the build, naming it. What comes back writes over what is here.

- One build per working folder and host at a time; a second is refused, *A build of <project> is
  already running on <host>* (call), held by a lock file on the laptop.
- Stopping the caller ends the ssh; whether the command on the host dies with it is best effort
  and goes to known gaps (call).
- It uses the person's own ssh with the options the attach uses (`BatchMode`, keepalives), and the
  host `local` runs the same steps without ssh, so tests and the owner can try it with no machine.

### Three ways in

- **The person, and the check command:** `onehand --build <host> -- <command…>`, run from inside
  the project or one of its worktrees. It is answered in `main` before the instance lock, as
  `--mcp-proxy` is, so a running onehand never takes it for a second launch. It needs no running
  window: it reads the config, finds the project the current folder belongs to, prints the
  output as it comes and exits with the build's code. A folder that is a project in several
  workspaces is read from the first, in name order, whose entry names that host (call). The check
  command needs nothing more than this line typed into its field.
- **A session's agent:** a tool, `build_on_host`, with `project` (the caller's when absent),
  `host` and `command`. It answers the exit code, the last 200 lines of output and what was
  brought back (call). `list_projects` appends ` — builds on: <hosts>` to a project's line when it has any, so an
  agent knows to ask; a line without build hosts reads as before (call).
- **Rules live in core.** The tool goes through the control drive like every other tool; the
  flag calls the same core function.

## Order of work

Three pull requests, each useful alone:

1. Declared hosts and Settings ▸ Hosts, the probe for both systems included. A Windows host can
   be declared from this PR on; its row says nothing builds on it yet.
2. Builds on a Linux host: the snapshot, the flag, the tool, artifacts, the project's build hosts.
3. Builds on a Windows host: the `receive-pack` form, `cmd.exe` paths and quoting.

Each updates its owning documents in the same PR: `docs/hosts.md`, `docs/control.md` for the tool,
`docs/screens.md` and `CONTEXT.md` for the new terms, `docs/known-gaps.md` for what is left out.

## Left to the builder

- The exact words of each refusal and each Settings row, within `docs/ui-patterns.md`.
- How the system probe tells Linux from Windows in one ssh call, and what it does with a Windows
  host whose default shell was changed to PowerShell.
- Whether a long `build_on_host` call outlives the agent's own tool timeout. If it does not, the
  tool answers how to run the flag from the agent's shell instead, and says so in its description.
- Quoting of the command for each shell. It is passed as the person wrote it; nothing in a path
  or host name onehand builds itself may reach a shell unquoted.
