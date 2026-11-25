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
use crate::task::drive::{link, Effect as RunEffect, Input as RunInput, Lane, RunDrive};
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

/// A run under way, and the session of each of its lanes.
pub(super) struct Driven {
    pub(super) run: RunDrive,
    pub(super) lanes: BTreeMap<Lane, Held>,
}

/// A lane's session, its uid, and what its agent was sent.
pub(super) struct Held {
    pub(super) session: SessionDrive,
    pub(super) uid: u64,
    rx: Rx,
}

impl Driven {
    fn current(&mut self) -> Option<&mut Held> {
        self.lanes.get_mut(self.run.current_lane())
    }
}

/// Something one drive said, still to be carried out.
enum Work {
    Tasks(TasksEffect<Win>),
    Run(String, RunEffect),
    /// What the session of a task's lane said.
    Session(String, Lane, SessionEffect),
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
    /// The sessions closed, by uid.
    pub(super) closed: Vec<u64>,
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
            closed: Vec::new(),
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

    /// Start `starting` off screen as a new task, as a tool would, and carry
    /// out what that says: the task's id.
    pub(super) fn start(&mut self, starting: crate::task::Starting) -> String {
        let id = self.new_id();
        let launch = Launch::off_screen(&[], starting, None, Some("a".into()));
        self.tasks(|t| t.start_off_screen(id.clone(), launch, WINDOW));
        self.settle();
        id
    }

    /// The agent of the session task `id` works in now says `event`.
    pub(super) fn agent(&mut self, id: &str, event: AcpEvent) {
        let driven = self.runs.get_mut(id).expect("a run under way");
        let lane = driven.run.current_lane().clone();
        let held = driven.current().expect("a session");
        let (mut fx, _) = held.session.apply(event, now());
        fx.extend(held.session.end_batch());
        self.queue.extend(
            fx.into_iter()
                .map(|e| Work::Session(id.to_string(), lane.clone(), e)),
        );
        self.settle();
    }

    /// What the agent of the session task `id` works in now was sent since
    /// this was last asked.
    pub(super) fn sent(&mut self, id: &str) -> Vec<AcpRequest> {
        let lane = self.runs[id].run.current_lane().clone();
        self.sent_in(id, &lane)
    }

    /// What the agent of task `id`'s `lane` was sent since this was last
    /// asked.
    pub(super) fn sent_in(&mut self, id: &str, lane: &Lane) -> Vec<AcpRequest> {
        let held = self.runs.get_mut(id).expect("a run under way");
        let held = held.lanes.get_mut(lane).expect("a session in that lane");
        std::iter::from_fn(|| held.rx.try_recv().ok()).collect()
    }

    /// Carry out everything said, and everything that says in turn, until
    /// nothing is left.
    pub(super) fn settle(&mut self) {
        while let Some(work) = self.queue.pop_front() {
            match work {
                Work::Tasks(effect) => self.carry_tasks(effect),
                Work::Run(id, effect) => self.carry_run(&id, effect),
                Work::Session(id, lane, effect) => self.carry_session(&id, &lane, effect),
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
            TasksEffect::CloseSession { uids, .. } => self.closed.extend(uids),
            TasksEffect::GiveRoom { task } => self.run_input(&task, RunInput::RoomGiven),
            // No board is played in these flows.
            TasksEffect::RoomFreed { .. } => {}
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
        let mut held = self.held();
        let Start { run, first, pinned } = start;
        let (run, out) = link::Joined::start(
            id.clone(),
            run,
            first,
            pinned,
            now(),
            Some(&mut held.session),
        );
        // The first session is the lane of the step the run starts at.
        let lane = run.current_lane().clone();
        let lanes = BTreeMap::from([(lane.clone(), held)]);
        self.runs.insert(id.clone(), Driven { run, lanes });
        self.carry_out(&id, &lane, out);
        self.tasks_input(TasksInput::Driven {
            task: id,
            started: Ok(()),
        });
    }

    /// A session whose agent is up, with the next uid.
    fn held(&mut self) -> Held {
        let (session, rx) = session();
        self.next_uid += 1;
        Held {
            session,
            uid: self.next_uid,
            rx,
        }
    }

    /// Tell task `id`'s run `input`, joined with its current lane's session,
    /// or, a person's prompt in another lane, with that lane's.
    fn run_input(&mut self, id: &str, input: RunInput) {
        let Some(driven) = self.runs.get_mut(id) else {
            return;
        };
        let lane = match &input {
            RunInput::LaneAppended { lane } => lane.clone(),
            _ => driven.run.current_lane().clone(),
        };
        let session = driven.lanes.get_mut(&lane).map(|held| &mut held.session);
        let out = link::Joined::new(&mut driven.run, session).handle(input, now());
        self.carry_out(id, &lane, out);
    }

    /// Queue what a joined call said to do: `lane`'s session's, then the
    /// run's.
    fn carry_out(&mut self, id: &str, lane: &Lane, out: link::Out) {
        self.queue.extend(
            out.session
                .into_iter()
                .map(|e| Work::Session(id.to_string(), lane.clone(), e)),
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
            RunEffect::AskRoom { key, limit } => self.tasks(|t| t.ask_room(id, key, limit)),
            RunEffect::LeaveRoom => self.tasks(|t| t.leave_room(id)),
            RunEffect::Finish { .. } => {
                let driven = self.runs.remove(id).expect("a run under way");
                let held = driven.lanes.get(driven.run.current_lane());
                let chat = held.map(|held| &*held.session as &Chat);
                let uids = driven.lanes.values().map(|held| held.uid).collect();
                self.tasks(|t| tasks_link::finished(t, &driven.run, chat, uids, WINDOW));
            }
            // As the shell does, the lane's session opened on its agent, up.
            RunEffect::OpenSession { lane, .. } => {
                let held = self.held();
                let driven = self.runs.get_mut(id).expect("a run under way");
                driven.lanes.insert(lane.clone(), held);
                self.run_input(
                    id,
                    RunInput::LaneOpened {
                        lane,
                        result: Ok(()),
                    },
                );
            }
            RunEffect::RunCommand { .. }
            | RunEffect::CallOffCommand
            | RunEffect::Push { .. }
            | RunEffect::FindPullRequest
            | RunEffect::OpenPullRequest
            | RunEffect::ReadLogs { .. }
            | RunEffect::MarkReady { .. } => {
                panic!("these flows run no command and use no forge")
            }
        }
    }

    /// Carry `effect` of task `id`'s `lane`'s session: what a lane the run
    /// is not working in says reaches it only as that lane's.
    fn carry_session(&mut self, id: &str, lane: &Lane, effect: SessionEffect) {
        let Some(driven) = self.runs.get_mut(id) else {
            return;
        };
        let Some(held) = driven.lanes.get(lane) else {
            return;
        };
        let session = &held.session;
        match effect {
            SessionEffect::Signal(signal) => {
                if let Some(input) = link::said(&signal, lane, &driven.run, session) {
                    self.run_input(id, input);
                }
            }
            SessionEffect::Changed => {
                if let Some(input) = link::notified(&driven.run, lane, session) {
                    self.run_input(id, input);
                }
            }
            SessionEffect::Request(request) => {
                if let Some(tx) = session.agent_channel() {
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
            | SessionEffect::Tell { .. }
            | SessionEffect::Unsent(_) => {}
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
