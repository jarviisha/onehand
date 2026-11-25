# Board

A project's board: the issues of the project's own tracker laid out as work, and a team of tasks
that pulls them one by one. A person talks an issue through with an agent, moves it to *To do*,
and a task of the board's workflow starts on it when the team has room.

A board is set up per project, in the project's settings, and exists only for a project that runs
on this machine. A project on a host has none: its issues are not kept here.

## What a person gets

- **Binding.** *Settings ▸ Projects ▸ Board* has a *Workflow* menu: *No board* and the library's
  workflows. A workflow that works in the checkout cannot be bound, since several tasks work at
  once and a checkout holds one; the reason is said under the menu and nothing is bound
  (`board::usable`). Once bound, *Team* sets how many tasks work at once, 1 to 8, 2 at first.
  Choosing *No board* on a board whose To do is not empty asks first (*Remove the board from
  <project>?*): its To do goes, the issues stay open in no list, and tasks already started run on.
- **The page.** *Board* in a project's dots menu, and the link on the project's page (*Board · N
  running · M to do*), open it. Columns side by side, scrolling sideways: *Backlog*, *To do*, a
  column per step of the workflow, *Done*. Each column draws at most `BOARD_SHOWN` rows and says
  how many more it left out. A task's row opens it on the Tasks page.
- **Backlog** is the project's open issues a brief can be made from (`Issues::workable`) that
  are neither in To do nor held by a task of the board (`board::backlog`). A person moves one to
  To do, and back.
- **To do** is in the person's order: first pulled first. A person moves an issue up or down in
  it, or back to the Backlog.
- **A step's column** holds the board's tasks at that step: working, or ended and needing
  attention. A task whose step left the workflow gets a column after the others, named by the
  step's id. **Done** holds finished ones, newest first.
- **Pulling.** When the team has room, the first issue in To do starts a task of the board's
  workflow, the brief made from the issue (`issues::supervised_brief_blocking`). The task is made
  when the issue is pulled, started off screen as a schedule's is (`task::start_off_screen`), so
  an issue in To do is not a task and is not on the Tasks page. The Tasks page's row reads *from
  the board · <workflow>*.
- **A refused pull** before its task is made (no brief, no worktree, no place) puts the issue back
  in the Backlog, with a toast naming it and the reason. It is not retried, and nothing is pulled
  in its place, so a reason every issue shares is said once.
- **A task that could not start once made** (no agent, no session) fails as an unwatched task does,
  said by its own start and no second toast. It holds its issue in its step's column, and nothing
  is pulled in its place.
- **An issue that can no longer be worked** (closed, deleted) is dropped from To do at the next
  look.

## When it ends

- **Done closes the issue** when the project's own tracker keeps it. An issue kept in step with a
  forge is left open: the pull request's merge closes it there (`board::after_end`).
- **A task a person stopped, took over or dismissed leaves the board**; its issue returns to the
  Backlog. A task that failed, ran out or timed out stays in its step's column, under *Needs
  attention*, holding its issue until the person acts on it.
- **A board task's end, like a schedule's, is unwatched:** it fails without a person looking, so
  it says what it came to on the Tasks page and nothing asks for a window.

## When it pulls

`board::pull` decides, over what the carrier read: as many of To do as the team has room for, the
issues that can no longer be worked dropped. `crates/app/src/board.rs` carries it out. A look
happens when:

- To do or the team changes, or a board is bound;
- a board task ends done, or a person stopped or took it over (`Signal::BoardEnded`, whose
  `pull` core decides). One that ended without finishing pulls nothing: the next would most
  likely end the same way, one after another until To do is empty;
- the tasks and the workflow library have both been read at boot.

And only while a window holds the project; there is none to start a task from otherwise. Opening
a window on the project does not pull (see [known-gaps.md](known-gaps.md)).

The starts on their way, issues taken out of To do whose task is not yet kept, are counted as
running so the team is not overfilled. They live in `ProjectsDrive` (`start_todo`, `landed`) and
are never written.

## What is kept

- **`project.toml`** holds the board with the project: `workflow`, `team` and `todo`, the issue
  numbers in order (`project::Board`, edited by `ProjectsDrive::{set_board, set_team, add_todo,
  move_todo, take_todo}`). A team size changed by the track is written once the changing stops.
- **A board task** is an ordinary task with `Origin::Board { project, issue }`, kept like any
  other. Where it stands comes from its last run and the app's view of whether it works
  (`board::columns`, `board::taken`, `board::running`).

**A restart** keeps To do, which only holds numbers, and pulls it again once the tasks and the
library are read. A task that had started comes back cut off, under *Needs attention*, for the
person's Resume; it is not counted as running, so the team has room for another.

## An older build

A build that does not know the board drops `board` from `project.toml` the next time it writes the
project, and skips the file of a board task it cannot read (`Origin::Board` is unknown to it). The
issues, which are the project's own, are untouched.
