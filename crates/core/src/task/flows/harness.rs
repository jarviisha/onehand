//! One front end's carriers, played with no window: every drive real, joined
//! by its links, and each effect carried once, its I/O done by core's own
//! blocking bodies on a scratch repository or answered as a front end would.
//! An effect these flows are not built to reach fails the test, so a flow
//! that starts reaching one is noticed; one with nothing to carry out here
//! (a write of no consequence, a clock, a redraw) is passed over.

use crate::acp::{AcpEvent, AcpRequest, ReqTx};
use crate::chat::drive::{Effect as SessionEffect, SessionDrive};
use crate::chat::{Chat, Now};
use crate::config::AgentSpec;
use crate::schedule::drive::{Effect as ScheduleEffect, Input as ScheduleInput, ScheduleDrive};
use crate::schedule::Schedule;
use crate::task::drive::{link, Effect as RunEffect, Input as RunInput, RunDrive};
use crate::task::tasks::link::{self as tasks_link, Seen};
use crate::task::tasks::{
    Begin, Effect as TasksEffect, Input as TasksInput, Placed, Start, TasksDrive,
};
use crate::task::{marks, queue, Launch};
use crate::unattended::drive::{
    blocking, Effect as UEffect, Input as UInput, On, UnattendedDrive, World,
};
use crate::unattended::link::{Joined, Out};
use crate::unattended::Project;
use crate::workflow::{Facts, Mark, Template};
use chrono::Utc;
use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime};

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
    Unattended(UEffect<Win>),
    Run(String, RunEffect),
    Session(String, SessionEffect),
    Schedule(ScheduleEffect<Win>),
}

/// Every drive a front end holds, and what reached a person or a file.
pub(super) struct Flow {
    pub(super) tasks: TasksDrive<Win>,
    pub(super) unattended: Option<UnattendedDrive>,
    pub(super) world: World,
    pub(super) schedule: ScheduleDrive<Win>,
    /// What the clock reads at its next look.
    pub(super) schedules: Vec<Schedule>,
    /// The projects unattended runs look at, each switched on.
    pub(super) roots: Vec<Project>,
    /// The runs under way, by task.
    pub(super) runs: BTreeMap<String, Driven>,
    /// Where each task asked for landed, as its asker heard it.
    pub(super) told: Vec<(String, Placed)>,
    /// The lines a run's session opened with, by task.
    pub(super) opened: BTreeMap<String, Vec<String>>,
    /// What a person or the log was told, as the drives said it.
    pub(super) said: Vec<String>,
    /// The project each ended unattended run kept or dropped.
    pub(super) torn_down: Vec<(PathBuf, bool)>,
    /// The task the clock's start under way is.
    starting: Option<String>,
    queue: VecDeque<Work>,
    next_id: u32,
    next_uid: u64,
    /// The clock, in seconds since the epoch.
    pub(super) now: u64,
}

impl Flow {
    /// Every drive, unattended runs set up when given, one agent `a`, and
    /// `library` on offer.
    pub(super) fn new(
        unattended: Option<UnattendedDrive>,
        library: Vec<Template>,
        now: u64,
    ) -> Self {
        let (schedule, _) = ScheduleDrive::new(now - 600);
        Self {
            tasks: TasksDrive::default(),
            unattended,
            world: World {
                library,
                agents: vec![AgentSpec {
                    name: "a".into(),
                    ..Default::default()
                }],
                offers: Vec::new(),
            },
            schedule,
            schedules: Vec::new(),
            roots: Vec::new(),
            runs: BTreeMap::new(),
            told: Vec::new(),
            opened: BTreeMap::new(),
            said: Vec::new(),
            torn_down: Vec::new(),
            starting: None,
            queue: VecDeque::new(),
            next_id: 0,
            next_uid: 0,
            now,
        }
    }

    fn on(&self) -> Vec<On> {
        self.roots.iter().map(|p| (p.clone(), false)).collect()
    }

    fn new_id(&mut self) -> String {
        self.next_id += 1;
        format!("t{}", self.next_id)
    }

    /// Join the tasks, the runs and unattended runs for one call, and keep
    /// what they say to carry out.
    fn joined(&mut self, call: impl FnOnce(&mut Joined<'_, Win>, &[On], &World) -> Out<Win>) {
        let roots = self.on();
        let Flow {
            tasks,
            unattended,
            runs,
            world,
            ..
        } = self;
        let mut joined = Joined::new(tasks, runs.values().map(|d| &d.run), unattended.as_mut());
        let out = call(&mut joined, &roots, world);
        self.keep(out);
    }

    fn keep(&mut self, out: Out<Win>) {
        self.queue.extend(out.tasks.into_iter().map(Work::Tasks));
        self.queue
            .extend(out.unattended.into_iter().map(Work::Unattended));
    }

    fn tasks_input(&mut self, input: TasksInput<Win>) {
        self.joined(|j, _, _| j.tasks(|t| ((), t.handle(input))).1);
    }

    fn unattended_input(&mut self, input: UInput<Win>) {
        self.joined(|j, roots, world| {
            j.unattended(|u, seen| {
                (
                    (),
                    u.handle(input, seen, roots.to_vec(), world, SystemTime::UNIX_EPOCH),
                )
            })
            .1
        });
    }

    fn schedule_input(&mut self, input: ScheduleInput<Win>) {
        let fx = self.schedule.handle(input, self.now, &Utc);
        self.queue.extend(fx.into_iter().map(Work::Schedule));
    }

    /// The clock comes round.
    pub(super) fn clock(&mut self) {
        self.schedule_input(ScheduleInput::Elapsed);
        self.settle();
    }

    /// Unattended runs' clock comes round.
    pub(super) fn tick(&mut self) {
        self.joined(|j, roots, world| {
            j.unattended(|u, seen| ((), u.tick(None, seen, roots.to_vec(), world)))
                .1
        });
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
                Work::Unattended(effect) => self.carry_unattended(effect),
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
            TasksEffect::Cut { .. } => panic!("these flows start on the checkout"),
            TasksEffect::ReadReview { .. } | TasksEffect::ConfirmAnswer { .. } => {
                panic!("these flows answer no review")
            }
            TasksEffect::JudgeRemoval { .. }
            | TasksEffect::ConfirmRemoval { .. }
            | TasksEffect::RemoveWorktree { .. } => panic!("these flows remove no worktree"),
            TasksEffect::Keep {
                task,
                launch,
                window,
            } => self.tasks_input(TasksInput::Kept {
                task,
                launch,
                window,
                open: true,
            }),
            TasksEffect::CloseSession { .. } => {}
            TasksEffect::Signal(signal) => self.said.push(format!("{signal:?}")),
            TasksEffect::Refresh => {}
            TasksEffect::Ask { .. }
            | TasksEffect::Asked(_)
            | TasksEffect::IssueEnded(_)
            | TasksEffect::Deliver(_)
            | TasksEffect::DeliverAll => panic!("the link's to route"),
        }
    }

    /// Drive task `id`, which holds its place, as the shell does: a session
    /// of its own, its opening lines, and the run started on it.
    fn drive(&mut self, id: String) {
        let there = self
            .tasks
            .resumable_run(&id)
            .is_some_and(|run| run.setup.dir.is_dir());
        let (start, opening) = match self.tasks.begin(&id, there) {
            Begin::Session { start, opening, .. } => (start, opening),
            Begin::Check(_) => panic!("these flows run no check"),
            // As the shell does: the drive is answered at once, and the
            // refusal handed on once that call is over.
            Begin::Refused { why, kind } => {
                self.tasks_input(TasksInput::Driven {
                    task: id.clone(),
                    started: Err(why.clone()),
                });
                return self.joined(|j, _, _| j.tasks(|t| ((), t.not_started(&id, why, kind))).1);
            }
        };
        let (mut session, rx) = session();
        self.opened.insert(id.clone(), opening);
        let issue = self.tasks.task(&id).is_some_and(|t| t.issue().is_some());
        let Start { run, first, pinned } = start;
        let (run, fx, asked) = link::start(
            id.clone(),
            run,
            first,
            pinned,
            issue,
            Instant::now(),
            Some(&mut session),
        );
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
        self.queue
            .extend(asked.into_iter().map(|e| Work::Session(id.clone(), e)));
        self.queue
            .extend(fx.into_iter().map(|e| Work::Run(id.clone(), e)));
        self.tasks_input(TasksInput::Driven {
            task: id,
            started: Ok(()),
        });
    }

    fn run_input(&mut self, id: &str, input: RunInput) {
        let Some(driven) = self.runs.get_mut(id) else {
            return;
        };
        let (fx, asked) = link::handle(
            &mut driven.run,
            input,
            Some(&mut driven.session),
            Instant::now(),
        );
        self.queue
            .extend(asked.into_iter().map(|e| Work::Session(id.to_string(), e)));
        self.queue
            .extend(fx.into_iter().map(|e| Work::Run(id.to_string(), e)));
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
            RunEffect::Submit(text) => {
                let driven = self.runs.get_mut(id).expect("a run under way");
                let (answer, fx) = link::submit(&mut driven.session, &text, now());
                self.queue
                    .extend(fx.into_iter().map(|e| Work::Session(id.to_string(), e)));
                if let Some(input) = answer {
                    self.run_input(id, input);
                }
            }
            RunEffect::CancelTurn => {
                let driven = self.runs.get_mut(id).expect("a run under way");
                let fx = link::cancel(&mut driven.session);
                self.queue
                    .extend(fx.into_iter().map(|e| Work::Session(id.to_string(), e)));
            }
            RunEffect::Note(note) => self.said.push(note),
            RunEffect::Arm { .. } | RunEffect::Refresh | RunEffect::SessionChanged => {}
            RunEffect::Defer(input) => self.run_input(id, input),
            RunEffect::Save => {
                let Flow { tasks, runs, .. } = self;
                let saved = tasks_link::save_run(tasks, &runs[id].run);
                assert!(saved.is_some(), "a run's write is its task's");
                self.run_input(id, RunInput::Saved(Ok(())));
            }
            RunEffect::Unattended(said) => {
                let id = id.to_string();
                let looks = said.looks();
                self.joined(|j, roots, world| {
                    j.run_said(&id, said, looks.then_some((roots.to_vec(), world)))
                });
            }
            RunEffect::Signal(signal) => self.said.push(format!("{signal:?}")),
            RunEffect::Finish { .. } => {
                let driven = self.runs.remove(id).expect("a run under way");
                let Flow {
                    tasks,
                    unattended,
                    runs,
                    ..
                } = self;
                let mut joined =
                    Joined::new(tasks, runs.values().map(|d| &d.run), unattended.as_mut());
                let chat: &Chat = &driven.session;
                let out = joined
                    .tasks(|t| {
                        (
                            (),
                            tasks_link::finished(t, &driven.run, Some((chat, driven.uid)), WINDOW),
                        )
                    })
                    .1;
                self.keep(out);
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

    fn carry_unattended(&mut self, effect: UEffect<Win>) {
        match effect {
            UEffect::Arm(_) | UEffect::Refresh => {}
            UEffect::Check {
                connectors,
                roots,
                prune,
            } => {
                let (accounts, checked) = crate::unattended::look_blocking(connectors, roots);
                self.unattended_input(UInput::Checked {
                    accounts,
                    checked,
                    prune,
                });
            }
            UEffect::Look(look) => {
                self.unattended_input(UInput::Looked(Box::new(look.run_blocking())))
            }
            UEffect::Claim {
                found,
                label,
                asked_from,
                held,
            } => {
                let begun = blocking::claim_found(*found, &label);
                let new_task = self.new_id();
                self.unattended_input(UInput::Claimed {
                    begun,
                    asked_from,
                    held,
                    holding: vec![(WINDOW, None)],
                    new_task,
                });
            }
            UEffect::Tell {
                tracker,
                repo,
                number,
                body,
            } => blocking::tell_blocking(&tracker, &repo, number, &body),
            UEffect::Send(sending) => {
                let landed = sending.send_blocking();
                self.unattended_input(UInput::Sent {
                    task: sending.task.clone(),
                    landed,
                });
            }
            UEffect::Teardown { dir, keep } => self.torn_down.push((dir, keep)),
            UEffect::NoteTakenUp {
                file,
                number,
                said,
                session,
            } => crate::issues::taken_up_blocking(&file, number, &said, session).unwrap(),
            UEffect::Tick => self.tick(),
            UEffect::Signal(signal) => self.said.push(format!("{signal:?}")),
            UEffect::Take { .. } | UEffect::ClaimPicked { .. } => {
                panic!("these flows pick nothing by hand")
            }
            UEffect::Submit { .. } | UEffect::AnswerReview { .. } | UEffect::Drain { .. } => {
                panic!("the link's to route")
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
                    library: self.world.library.clone(),
                };
                self.schedule_input(input);
            }
            ScheduleEffect::Start { starting, window } => {
                let id = self.new_id();
                self.starting = Some(id.clone());
                let launch = Launch::off_screen(&[], *starting, None, Some("a".into()));
                self.joined(|j, _, _| j.tasks(|t| ((), t.start_off_screen(id, launch, window))).1);
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
    Now {
        secs: crate::chat::now_secs(),
        at: Instant::now(),
    }
}

/// A scratch git repository on `main` with one commit, keeping its own
/// issues in `issues.json`, holding `issues` (a title and its labels).
pub(crate) fn repo(name: &str, issues: &[(&str, &[&str])]) -> Project {
    let root = std::env::temp_dir().join(format!("onehand-drive-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    git(&root, &["init", "-q", "-b", "main"]);
    git(&root, &["config", "user.email", "t@example.com"]);
    git(&root, &["config", "user.name", "T"]);
    std::fs::write(root.join("a.txt"), "x").unwrap();
    git(&root, &["add", "a.txt"]);
    git(&root, &["commit", "-qm", "one"]);
    let file = root.join("issues.json");
    for (title, labels) in issues {
        crate::issues::update_blocking(&file, |kept| {
            kept.create(
                crate::issues::Draft {
                    title: title.to_string(),
                    labels: labels.iter().map(|l| l.to_string()).collect(),
                    ..Default::default()
                },
                1,
            )
        })
        .unwrap();
    }
    Project {
        root,
        issues: Some(file),
    }
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
