//! One front end's carriers, played with no window: every drive real, joined
//! by its links, and each effect carried once, its I/O done by core's own
//! blocking bodies on a scratch repository or answered as a front end would.
//! An effect these flows are not built to reach fails the test, so a flow
//! that starts reaching one is noticed; one with nothing to carry out here
//! (a write of no consequence, a clock, a redraw) is passed over.

use crate::acp::{AcpEvent, AcpRequest, ReqTx};
use crate::chat::drive::{Effect as SessionEffect, SessionDrive};
use crate::chat::Chat;
use crate::schedule::drive::{Effect as ScheduleEffect, Input as ScheduleInput, ScheduleDrive};
use crate::schedule::Schedule;
use crate::task::drive::{link, Effect as RunEffect, Input as RunInput, RunDrive};
use crate::task::tasks::link::{self as tasks_link, Seen};
use crate::task::tasks::{
    Begin, Effect as TasksEffect, Input as TasksInput, Placed, Start, TasksDrive,
};
use crate::task::{marks, queue, Launch};
use crate::workflow::{Facts, Mark, Template};
use crate::workspace::WorkspaceDrive;
use crate::Now;
use chrono::Utc;
use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};

/// A window, as a front end names it.
pub(super) type Win = u8;
pub(super) const WINDOW: Win = 1;

type Rx = tokio::sync::mpsc::UnboundedReceiver<AcpRequest>;

/// A run under way, its session, and what the session's agent was sent.
pub(super) struct Driven {
    pub(super) run: RunDrive,
    pub(super) session: SessionDrive,
    pub(super) uid: u64,
    pub(super) rx: Rx,
}

/// Something one drive said, still to be carried out.
enum Work {
    Tasks(TasksEffect<Win>),
    Run(String, RunEffect),
    Session(String, SessionEffect),
    Schedule(ScheduleEffect<Win>),
}

/// Every drive a front end holds, and what reached a person or a file.
pub(super) struct Flow {
    pub(super) tasks: TasksDrive<Win>,
    /// The workflows on offer.
    pub(super) library: Vec<Template>,
    pub(super) schedule: ScheduleDrive<Win>,
    /// The workspace of the one window, [`WINDOW`].
    pub(super) workspace: WorkspaceDrive,
    /// What the clock reads at its next look.
    pub(super) schedules: Vec<Schedule>,
    /// The runs under way, by task.
    pub(super) runs: BTreeMap<String, Driven>,
    /// Where each task asked for landed, as its asker heard it.
    pub(super) told: Vec<(String, Placed)>,
    /// What a person or the log was told, as the drives said it.
    pub(super) said: Vec<String>,
    /// The task the clock's start under way is.
    starting: Option<String>,
    queue: VecDeque<Work>,
    next_id: u32,
    next_uid: u64,
    /// The clock, in seconds since the epoch.
    pub(super) now: u64,
}

impl Flow {
    /// Every drive, and `library` on offer.
    pub(super) fn new(library: Vec<Template>, now: u64) -> Self {
        let (schedule, _) = ScheduleDrive::new(Now {
            secs: now - 600,
            ..self::now()
        });
        Self {
            tasks: TasksDrive::default(),
            library,
            schedule,
            workspace: WorkspaceDrive::new(crate::workspace::Workspace::seeded("/")),
            schedules: Vec::new(),
            runs: BTreeMap::new(),
            told: Vec::new(),
            said: Vec::new(),
            starting: None,
            queue: VecDeque::new(),
            next_id: 0,
            next_uid: 0,
            now,
        }
    }

    fn new_id(&mut self) -> String {
        self.next_id += 1;
        format!("t{}", self.next_id)
    }

    /// Give the tasks' drive `command`, and keep what it says to carry out.
    fn tasks(&mut self, command: impl FnOnce(&mut TasksDrive<Win>) -> Vec<TasksEffect<Win>>) {
        let fx = command(&mut self.tasks);
        self.queue.extend(fx.into_iter().map(Work::Tasks));
    }

    fn tasks_input(&mut self, input: TasksInput<Win>) {
        self.tasks(|t| t.handle(input));
    }

    fn schedule_input(&mut self, input: ScheduleInput<Win>) {
        let fx = self.schedule.handle(
            input,
            Now {
                secs: self.now,
                ..now()
            },
            &Utc,
        );
        self.queue.extend(fx.into_iter().map(Work::Schedule));
    }

    /// The clock comes round.
    pub(super) fn clock(&mut self) {
        self.schedule_input(ScheduleInput::Elapsed);
        self.settle();
    }

    /// The agent of task `id`'s session says `event`.
    pub(super) fn agent(&mut self, id: &str, event: AcpEvent) {
        let driven = self.runs.get_mut(id).expect("a run under way");
        let (mut fx, _) = driven.session.apply(event, now());
        fx.extend(driven.session.end_batch());
        self.queue
            .extend(fx.into_iter().map(|e| Work::Session(id.to_string(), e)));
        self.settle();
    }

    /// What task `id`'s session's agent was sent since this was last asked.
    pub(super) fn sent(&mut self, id: &str) -> Vec<AcpRequest> {
        let driven = self.runs.get_mut(id).expect("a run under way");
        std::iter::from_fn(|| driven.rx.try_recv().ok()).collect()
    }

    /// Carry out everything said, and everything that says in turn, until
    /// nothing is left.
    pub(super) fn settle(&mut self) {
        while let Some(work) = self.queue.pop_front() {
            match work {
                Work::Tasks(effect) => self.carry_tasks(effect),
                Work::Run(id, effect) => self.carry_run(&id, effect),
                Work::Session(id, effect) => self.carry_session(&id, effect),
                Work::Schedule(effect) => self.carry_schedule(effect),
            }
        }
    }

    fn carry_tasks(&mut self, effect: TasksEffect<Win>) {
        match effect {
            TasksEffect::Save(_) => self.tasks_input(TasksInput::Saved(Ok(()))),
            TasksEffect::SaveDismissed(task) => self.tasks_input(TasksInput::Dismissed {
                task: task.id,
                written: Ok(()),
            }),
            TasksEffect::Remove(_) | TasksEffect::DropMarks { .. } => {}
            TasksEffect::FindPlace { task, dir, window } => {
                self.tasks_input(TasksInput::PlaceFound {
                    task,
                    place: queue::place_blocking(&dir),
                    window,
                    window_open: true,
                })
            }
            TasksEffect::Drive { task, .. } => self.drive(task),
            TasksEffect::WatchDrain { task, .. } => self.tasks_input(TasksInput::Drained(task)),
            TasksEffect::Pin {
                task,
                dir,
                refs,
                from,
            } => self.tasks_input(TasksInput::Pinned {
                task,
                from,
                pinned: marks::pin_blocking(&dir, &refs),
            }),
            TasksEffect::Defer(input) => self.tasks_input(input),
            TasksEffect::Told { task, placed } => {
                if self.starting.as_deref() == Some(task.as_str()) {
                    self.starting = None;
                    self.schedule_input(ScheduleInput::Started(Ok((task.clone(), placed.clone()))));
                }
                self.told.push((task, placed));
            }
            TasksEffect::Cut(start) => {
                let made =
                    crate::task::cut_blocking(start.launch.repo(), start.launch.title(), None, &[]);
                self.tasks_input(TasksInput::Cut { start, made });
            }
            TasksEffect::Keep(start) => {
                // The workspace's own effects (its write, a git sweep) are its
                // carrier's, not a flow's.
                let Flow {
                    tasks, workspace, ..
                } = self;
                let dir = start.launch.dir().to_path_buf();
                let project =
                    crate::project::Project::new(crate::project::ProjectId::mint(1), None, dir);
                let (fx, _) = tasks_link::keep(tasks, Some(workspace), Some(&project), start);
                self.queue.extend(fx.into_iter().map(Work::Tasks));
            }
            TasksEffect::JudgeRemoval(_)
            | TasksEffect::ConfirmRemoval { .. }
            | TasksEffect::RemoveWorktree { .. } => panic!("these flows remove no worktree"),
            TasksEffect::CloseSession { .. } => {}
            TasksEffect::Signal(signal) => self.said.push(format!("{signal:?}")),
            TasksEffect::Refresh => {}
        }
    }

    /// Drive task `id`, which holds its place, as the shell does: a session
    /// of its own, and the run started on it.
    fn drive(&mut self, id: String) {
        let there = self
            .tasks
            .resumable_run(&id)
            .is_some_and(|run| run.setup.dir.is_dir());
        let start = match self.tasks.begin(&id, there, crate::Now::read()) {
            Begin::Session { start, .. } => start,
            Begin::Check(_) => panic!("these flows run no check"),
            // As the shell does: the drive is answered at once, and the
            // refusal handed on once that call is over.
            Begin::Refused { why, kind } => {
                self.tasks_input(TasksInput::Driven {
                    task: id.clone(),
                    started: Err(why.clone()),
                });
                return self.tasks(|t| t.not_started(&id, why, kind, crate::Now::read()));
            }
        };
        let (mut session, rx) = session();
        let Start { run, first, pinned } = start;
        let (run, out) =
            link::Joined::start(id.clone(), run, first, pinned, now(), Some(&mut session));
        self.next_uid += 1;
        self.runs.insert(
            id.clone(),
            Driven {
                run,
                session,
                uid: self.next_uid,
                rx,
            },
        );
        self.carry_out(&id, out);
        self.tasks_input(TasksInput::Driven {
            task: id,
            started: Ok(()),
        });
    }

    fn run_input(&mut self, id: &str, input: RunInput) {
        let Some(driven) = self.runs.get_mut(id) else {
            return;
        };
        let out =
            link::Joined::new(&mut driven.run, Some(&mut driven.session)).handle(input, now());
        self.carry_out(id, out);
    }

    /// Queue what a joined call said to do: the session's, then the run's.
    fn carry_out(&mut self, id: &str, out: link::Out) {
        self.queue.extend(
            out.session
                .into_iter()
                .map(|e| Work::Session(id.to_string(), e)),
        );
        self.queue
            .extend(out.run.into_iter().map(|e| Work::Run(id.to_string(), e)));
    }

    fn carry_run(&mut self, id: &str, effect: RunEffect) {
        if !self.runs.contains_key(id) {
            return;
        }
        match effect {
            RunEffect::Pin { dir, refs } => {
                self.run_input(id, RunInput::Pinned(marks::pin_blocking(&dir, &refs)))
            }
            RunEffect::Measure { dir } => {
                self.run_input(id, RunInput::Measured(Mark::read_blocking(&dir)))
            }
            RunEffect::ReadFacts { dir, from } => {
                self.run_input(id, RunInput::TurnRead(Facts::read_blocking(&dir, &from)))
            }
            RunEffect::Arm { .. } | RunEffect::Refresh | RunEffect::SessionChanged => {}
            RunEffect::Defer(input) => self.run_input(id, input),
            RunEffect::Save => {
                let Flow { tasks, runs, .. } = self;
                let saved = tasks_link::save_run(tasks, &runs[id].run);
                assert!(saved.is_some(), "a run's write is its task's");
                self.run_input(id, RunInput::Saved(Ok(())));
            }
            RunEffect::Signal(signal) => self.said.push(format!("{signal:?}")),
            RunEffect::Finish { .. } => {
                let driven = self.runs.remove(id).expect("a run under way");
                let chat: &Chat = &driven.session;
                self.tasks(|t| {
                    tasks_link::finished(t, &driven.run, Some((chat, driven.uid)), WINDOW)
                });
            }
            RunEffect::RunCommand { .. }
            | RunEffect::CallOffCommand
            | RunEffect::Push { .. }
            | RunEffect::FindPullRequest
            | RunEffect::OpenPullRequest
            | RunEffect::ReadLogs { .. }
            | RunEffect::MarkReady { .. } => panic!("these flows run no command and use no forge"),
        }
    }

    fn carry_session(&mut self, id: &str, effect: SessionEffect) {
        let Some(driven) = self.runs.get_mut(id) else {
            return;
        };
        match effect {
            SessionEffect::Signal(signal) => {
                if let Some(input) = link::input_of(&signal, &driven.run, &driven.session) {
                    self.run_input(id, input);
                }
            }
            SessionEffect::Changed => {
                if let Some(input) = link::notified(&driven.run, &driven.session) {
                    self.run_input(id, input);
                }
            }
            SessionEffect::Request(request) => {
                if let Some(tx) = driven.session.agent_channel() {
                    let _ = tx.send(request);
                }
            }
            // A transcript's writes, its clocks, and what a person is told
            // of the session are the session's carrier's, not a flow's.
            SessionEffect::OfferChanged
            | SessionEffect::Save(_)
            | SessionEffect::Arm { .. }
            | SessionEffect::Disarm
            | SessionEffect::SaveTitle { .. }
            | SessionEffect::Look(_)
            | SessionEffect::Tell { .. } => {}
            SessionEffect::SendCode { .. } | SessionEffect::PromptAgain => {
                panic!("these flows work here, and send nothing again")
            }
        }
    }

    fn carry_schedule(&mut self, effect: ScheduleEffect<Win>) {
        match effect {
            ScheduleEffect::Arm(_) | ScheduleEffect::Refresh => {}
            ScheduleEffect::Read => {
                let Flow {
                    tasks,
                    runs,
                    schedule,
                    ..
                } = &*self;
                let seen = Seen::new(tasks, runs.values().map(|d| &d.run));
                let working = schedule
                    .last_tasks()
                    .filter(|task| seen.busy(task))
                    .map(str::to_string)
                    .collect();
                let holders = self
                    .schedules
                    .iter()
                    .map(|s| (s.project.clone(), WINDOW))
                    .collect();
                let input = ScheduleInput::Read {
                    all: Ok(self.schedules.clone()),
                    holders,
                    working,
                    library: self.library.clone(),
                };
                self.schedule_input(input);
            }
            ScheduleEffect::Start { starting, window } => {
                let id = self.new_id();
                self.starting = Some(id.clone());
                let launch = Launch::off_screen(&[], *starting, None, Some("a".into()));
                self.tasks(|t| t.start_off_screen(id, launch, window));
            }
            ScheduleEffect::Write { looked } => {
                self.schedules = looked.clone();
                self.schedule_input(ScheduleInput::Written(Ok(looked)));
            }
            ScheduleEffect::Signal(signal) => self.said.push(format!("{signal:?}")),
            ScheduleEffect::Switch { .. } => panic!("these flows switch nothing"),
        }
    }
}

/// A session whose agent is up, and what it is sent.
fn session() -> (SessionDrive, Rx) {
    let mut session = SessionDrive::new(Chat::default());
    let (tx, rx): (ReqTx, _) = tokio::sync::mpsc::unbounded_channel();
    session.apply(
        AcpEvent::Connected {
            tx,
            resumed: false,
            forkable: false,
        },
        now(),
    );
    session.end_batch();
    (session, rx)
}

pub(super) fn now() -> Now {
    Now::read()
}

/// A scratch git repository on `main` with one commit.
pub(crate) fn repo(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("onehand-drive-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    git(&root, &["init", "-q", "-b", "main"]);
    git(&root, &["config", "user.email", "t@example.com"]);
    git(&root, &["config", "user.name", "T"]);
    std::fs::write(root.join("a.txt"), "x").unwrap();
    git(&root, &["add", "a.txt"]);
    git(&root, &["commit", "-qm", "one"]);
    root
}

/// Run git in `dir`, which must succeed.
pub(crate) fn git(dir: &Path, args: &[&str]) {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .expect("git must be installed to run this test");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}
