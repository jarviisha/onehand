# Schedules

Work onehand starts by the clock, while it runs. A schedule names a project, what it starts there,
the agent and how it runs, what its agent may use of onehand's tools, and when, as a cron
expression. Every start is a task like any other: on the Tasks page, through the queue, driven off
screen.

**Only while onehand is open.** There is no background process and no system cron: a schedule
whose time came while onehand was closed runs once when it next opens, or waits for its next time,
as it says. A schedule whose project no window holds waits until one does.

**No schedule answers a card, a permission or an approval.** Its agent may be started in a mode of
its own that asks less; what it does ask waits for a person.

## The file

`<config dir>/schedules.toml`, one `[[schedule]]` table each, read again at every look, so a
schedule edited in the file holds from the next one. The *Schedules* page on the rail lists them
(filtered by project; a project's page has a *Schedules* box leading there) and switches each on
or off; switched on, nothing due before then is caught up. *New schedule* and a row open the form
that makes, edits and deletes one; its save patches the file by id, through the same writer the
clock writes through. Deleting one keeps the tasks it started.

```toml
[[schedule]]
id = "1792731416478576221"
name = "Nightly code review"
project = "/home/me/code/app"
on = true
when = "0 9 * * *"          # minute hour day-of-month month day-of-week
agent = "Claude Code"       # left out: the first agent
mode = "acceptEdits"        # left out: the agent's own
tools = ["list_issues", "create_issue"]
labels = []                 # "auto" hands every issue it writes to unattended runs
missed = "run_once"         # or "skip"
overlap = "skip"            # or "queue"

[schedule.options]
model = "opus"

[schedule.starts]
kind = "prompt"             # or kind = "workflow", workflow = "<id>", title = "...", body = "..."
prompt = "/review the codebase; call list_issues first, then create_issue for each new problem"
```

Kept beside them by the clock: `on_since` (when it was switched on; nothing due before it is caught
up, and a schedule switched on by hand with none counts from when onehand opened), `last_fired`
and `said` (what its last look came to when that was a skip or a start that failed).

## When

Five fields: minute, hour, day of month, month, day of week (0 and 7 Sunday); `*`, numbers,
ranges `a-b`, lists `a,b` and steps `*/n`, `a-b/n` and `a/n` (from `a` to the field's end). When
day of month and day of week are both restricted, a day matching either is a match; a field
written from `*` counts as `*` for that rule. In the machine's own time zone; across a
daylight-saving change a time can come twice or not at all.

## The clock

One look five seconds after start, then every minute. Each look, for every schedule switched on
whose project a window holds (the first window that holds it):

| It comes to | What happens |
|---|---|
| nothing due since it last fired or was switched on | nothing |
| due, its last task not queued, running or waiting | it starts |
| due while onehand was closed | it starts once, however many times it was missed, or is skipped (`missed = "skip"`) |
| due while its last task is still queued, running or waiting | it is skipped (`overlap = "skip"`), or starts once that task is done (`overlap = "queue"`) |

Anything but nothing moves `last_fired`, so one due time fires once. A skip is said in `said`.
The clock writes only `last_fired` and `said`, onto the file as it is when it writes, so a hand
edit made during a look stands. A time held for a schedule that is then switched off is let go.

## A start

- **A prompt** runs as a task of a workflow of one agent step, *Prompt*, whose prompt is the
  brief and the brief the schedule's prompt (a skill is used by naming it there). It
  works on the project's checkout. It is said as a prompt, not a workflow: its row and its strip
  name *Prompt* once and never its one step, its transcript tells no step, and it ends *Done*, or
  *Stopped* with why.
- **A workflow** runs the library's workflow of that id on the schedule's title and body, on the
  checkout or a worktree as the workflow says.
- **The agent** is the schedule's for every step that names none; **its mode and settings** are
  every agent step's that sets none, setting by setting, and a step's own win. What blocks the
  launcher's *Run* blocks it, in the preflight's words.
- **What its sessions may use**: the schedule's tools and labels are copied onto the task, and
  every drive of it gives its session that set (`docs/control.md`), a Resume or a Retry included;
  editing or deleting the schedule never widens what an earlier task may do.
- **A start that cannot go** says why in `said`, and on the desktop once per reason; it tries again
  at its next time. The schedule is never switched off by itself. A task that waited for its place
  and cannot start when its turn comes ends failed, as an issue's run does, so its row and the
  desktop say why.
- **Its task says where it came from**: *<schedule> · <workflow>* on the Tasks page.
- **When it ends**, a desktop notification names the schedule and how it ended.
- **A run that ends done closes its session**: it leaves the rail, and its conversation, named for
  the task, stays under past conversations. One that needs attention, failed or was stopped keeps
  its session in the rail. The session on screen stays open, so nothing the person is looking at
  moves; nothing else is shown and the caret stays where it was.
