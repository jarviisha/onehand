//! Unattended runs, as the app drives them: the tick, the live run, the
//! watching, the timeout and the teardown.
//!
//! Everything that can be decided without a window — which issue, what branch,
//! what the prompt says, what the issue is told — is `onehand_core::unattended`.
//! What is here is what needs an entity, a window or a timer.
//!
//! **One per process, on [`Shared`]**, for the reason the remote bridge is
//! there: two windows each running a tick would be two agents on one issue. The
//! tick therefore belongs to no window and asks each in turn for its projects.

use crate::chat::session::{ChatEvent, ChatSession};
use crate::state::Shared;
use gpui::{App, BorrowAppContext as _, Entity, Subscription, Task, WeakEntity};
use onehand_core::chat::UserAsk;
use onehand_core::config::{AgentSpec, UnattendedConfig};
use onehand_core::unattended::{self as core, Ending, GitHub, Issue};
use onehand_core::worktree;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

/// How long a cancelled turn is given to wind down before the run is settled
/// anyway. Cancelling asks the adapter to end the turn, and the turn ending is
/// what writes its transcript — closing the session straight away would lose
/// the one turn the run was about.
const WIND_DOWN: Duration = Duration::from_secs(30);

/// The unattended half of the process: its settings, the live run and the tick.
pub struct Unattended {
    label: String,
    timeout: Duration,
    mode: String,
    agent: Option<String>,
    /// A claim is on its way to GitHub and the worktree is being made. A tick
    /// landing now must not start a second.
    claiming: bool,
    /// Why no run can start at all, whatever project is switched on: a config
    /// that cannot work (no label, an interval that does not parse, a mode the
    /// agent does not offer). Every run would fail the same way on a fresh
    /// issue, so the search stops, and every switched-on row says why.
    blocked: Option<String>,
    run: Option<Run>,
    /// What `gh` last said about being signed in, for the line in Settings.
    /// `None` until the first answer lands.
    github: Option<GitHub>,
    /// Why the last look at a project could not go ahead, by project.
    ///
    /// **Shown on the project's row**, because the alternative is stderr: a
    /// project that is not on GitHub, or a `gh` that is signed out, fails the
    /// same way on every tick, and a switch that is on while nothing can happen
    /// looks exactly like one that is working. An entry is cleared by the next
    /// look that gets through.
    problems: HashMap<PathBuf, String>,
    _tick: Task<()>,
}

/// One issue being worked.
struct Run {
    claimed: Claimed,
    uid: u64,
    session: WeakEntity<ChatSession>,
    window: gpui::AnyWindowHandle,
    shell: WeakEntity<crate::shell::Shell>,
    prompted: bool,
    /// The turn has been cancelled, and this is what the run ends as once it
    /// has wound down.
    ending: Option<Ending>,
    _watch: Subscription,
    /// Fires if the session goes without saying so — its window closed under
    /// it — so the run settles now rather than holding the tick until its
    /// timeout and then reporting the wrong ending.
    _release: Subscription,
    /// The run's timeout, and after a cancel the wind-down in its place.
    _clock: Task<()>,
}

/// Start the tick, or say why it cannot run.
///
/// **The state is filed either way.** A config that cannot work is not the same
/// as a feature that is off: the switches are still on screen and still
/// switchable, so the reason has to be somewhere the rows and Settings can read
/// it — left only on stderr, a bad interval read on screen as a missing label,
/// and the GitHub line waited for an answer that was never going to come. The
/// search is what stops; looking at projects and at `gh` still works.
pub fn boot(cfg: &UnattendedConfig, cx: &mut App) {
    let label = cfg.label.trim().to_string();
    let every = core::parse_every(&cfg.every);
    let timeout = core::parse_every(&cfg.timeout);
    let blocked = if label.is_empty() {
        Some("no label is set in the config (unattended.label), so nothing is picked up".into())
    } else if every.is_none() || timeout.is_none() {
        Some(format!(
            "unattended.every and unattended.timeout take a number and a unit (\"30m\", \
             \"2h\", \"90s\"); the config has {:?} and {:?}",
            cfg.every, cfg.timeout
        ))
    } else {
        None
    };
    if let Some(why) = &blocked {
        eprintln!("onehand: unattended runs cannot start: {why}");
    }
    // The tick runs whatever the config says, because it is also what keeps the
    // rows' answers current; it only searches when nothing blocks a run.
    // The defaults are core's, parsed, so there is one answer to "how often".
    let fallback = UnattendedConfig::default();
    let every = every
        .or_else(|| core::parse_every(&fallback.every))
        .unwrap_or(Duration::from_secs(1800));
    let tick = cx.spawn(async move |cx| {
        loop {
            cx.background_executor().timer(every).await;
            cx.update(tick);
        }
    });
    cx.update_global::<Shared, _>(|shared, _| {
        shared.unattended = Some(Unattended {
            label,
            timeout: timeout
                .or_else(|| core::parse_every(&fallback.timeout))
                .unwrap_or(Duration::from_secs(2700)),
            mode: cfg.mode.clone(),
            agent: cfg.agent.clone(),
            claiming: false,
            blocked,
            run: None,
            github: None,
            problems: HashMap::new(),
            _tick: tick,
        });
    });
}

/// Why no run can start at all, if something stops every one.
pub fn blocked(cx: &App) -> Option<String> {
    Shared::global(cx).unattended.as_ref()?.blocked.clone()
}

/// What `gh` last said about being signed in, if it has answered yet.
pub fn github(cx: &App) -> Option<GitHub> {
    Shared::global(cx).unattended.as_ref()?.github.clone()
}

/// Why the last look at `root` could not go ahead, if it could not.
pub fn problem(root: &std::path::Path, cx: &App) -> Option<String> {
    Shared::global(cx)
        .unattended
        .as_ref()?
        .problems
        .get(root)
        .cloned()
}

/// Ask `gh` again, and look again at every project that is switched on —
/// what Settings' *Check again* does, and what a window does when it opens.
pub fn recheck(cx: &mut App) {
    let roots = opted_in_roots(cx);
    check(roots, true, cx);
}

/// Look at one project that was just switched on, so a switch that cannot work
/// says so now rather than at the next tick, half an hour away.
pub fn check_now(root: PathBuf, cx: &mut App) {
    check(vec![root], false, cx);
}

/// Ask `gh` who it is signed in as and look at `roots`, off the UI loop, then
/// file what was found. `prune` when `roots` is every switched-on project, so
/// what is kept for a project since switched off or closed goes with it.
fn check(roots: Vec<PathBuf>, prune: bool, cx: &mut App) {
    cx.spawn(async move |cx| {
        let (github, checked) = cx
            .background_executor()
            .spawn(async move { look_blocking(roots) })
            .await;
        cx.update(|cx| {
            with(cx, |u| u.github = Some(github));
            record(checked, prune, cx);
        });
    })
    .detach();
}

/// What `gh` says about being signed in, and for each of `roots` why it cannot
/// be worked, if it cannot: a remote not on GitHub (read locally, so a project
/// somewhere else costs no call to GitHub), or a `gh` that cannot be used.
fn look_blocking(roots: Vec<PathBuf>) -> (GitHub, Vec<(PathBuf, Option<String>)>) {
    let github = core::github_blocking();
    let checked = roots
        .into_iter()
        .map(|root| {
            let why = core::github_project_blocking(&root)
                .err()
                .or_else(|| github.problem());
            (root, why)
        })
        .collect();
    (github, checked)
}

/// File what each look found, and redraw the rows that show it. A later entry
/// for the same project wins, so a search that failed after its project passed
/// the look is what the row says.
///
/// `prune` drops what is kept for projects no longer switched on — read now,
/// when the answer lands, and not from the list the look started with. A look
/// takes up to a minute, and a project switched on in that minute has already
/// been looked at by itself; clearing everything the slower look did not cover
/// threw that answer away.
fn record(checked: Vec<(PathBuf, Option<String>)>, prune: bool, cx: &mut App) {
    let switched_on = prune.then(|| opted_in_roots(cx));
    with(cx, |u| {
        if let Some(switched_on) = switched_on {
            u.problems.retain(|root, _| switched_on.contains(root));
        }
        for (root, why) in checked {
            match why {
                Some(why) => u.problems.insert(root, why),
                None => u.problems.remove(&root),
            };
        }
    });
    cx.refresh_windows();
}

/// Every switched-on project across every window, in rail order, once each.
fn opted_in_roots(cx: &App) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    let shells: Vec<_> = Shared::global(cx)
        .windows
        .iter()
        .map(|w| w.shell.clone())
        .collect();
    for shell in shells.iter().filter_map(WeakEntity::upgrade) {
        for root in shell.read(cx).unattended_roots() {
            if !roots.contains(&root) {
                roots.push(root);
            }
        }
    }
    roots
}

/// The run in progress, as the project it came from and the issue it is on.
///
/// For the rail, which says on a project's row that a run is working one of its
/// issues: the run's own session is on a worktree's row of its own, and nothing
/// on the project the issue belongs to would otherwise say so.
pub fn live_run(cx: &App) -> Option<(PathBuf, u64)> {
    let run = Shared::global(cx).unattended.as_ref()?.run.as_ref()?;
    Some((run.claimed.repo.clone(), run.claimed.issue.number))
}

/// The label that asks for a run, for the places that tell the user which
/// label to put on an issue.
pub fn label(cx: &App) -> String {
    Shared::global(cx)
        .unattended
        .as_ref()
        .map(|u| u.label.clone())
        .unwrap_or_default()
}

/// Whether `uid` is the session of the run in progress.
pub fn is_run(uid: u64, cx: &App) -> bool {
    Shared::global(cx)
        .unattended
        .as_ref()
        .and_then(|u| u.run.as_ref())
        .is_some_and(|run| run.uid == uid)
}

/// Act on the unattended state, if there is one.
fn with<R>(cx: &mut App, act: impl FnOnce(&mut Unattended) -> R) -> Option<R> {
    cx.update_global::<Shared, _>(|shared, _| shared.unattended.as_mut().map(act))
}

/// Look at every switched-on project, and search them for an issue if
/// nothing is running and nothing blocks a run.
///
/// **Every tick looks, even while a run is live.** The rows show what the last
/// look found, and a tick that only looked while it was about to search left
/// them as old as the run was long — and left `gh` signed in on screen after it
/// had been signed out.
fn tick(cx: &mut App) {
    let roots = opted_in_roots(cx);
    // Nothing switched on is nothing to look at and nothing to search, so a
    // tick asks GitHub nothing at all — the feature costs nobody who has not
    // switched a project on.
    if roots.is_empty() {
        return;
    }
    let search = with(cx, |u| {
        let idle = !u.claiming && u.blocked.is_none() && u.run.is_none();
        idle.then(|| {
            u.claiming = true;
            u.label.clone()
        })
    })
    .flatten();
    cx.spawn(async move |cx| {
        let searching = search.is_some();
        // A panic in there would otherwise leave `claiming` set for the life of
        // the process, and no issue would be looked for again. Said and treated
        // as nothing found.
        let (github, checked, begun) = cx
            .background_executor()
            .spawn(async move {
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let (github, mut checked) = look_blocking(roots);
                    // Only the projects that passed the look are searched.
                    let workable: Vec<PathBuf> = checked
                        .iter()
                        .filter(|(_, why)| why.is_none())
                        .map(|(root, _)| root.clone())
                        .collect();
                    let begun =
                        search.and_then(|label| begin_blocking(&workable, &label, &mut checked));
                    (Some(github), checked, begun)
                }))
                .unwrap_or_else(|_| {
                    // The look, the search and the claim are all in here, and
                    // none of them has taken a label unless it finished.
                    eprintln!("onehand: looking for an unattended run panicked");
                    (None, Vec::new(), None)
                })
            })
            .await;
        cx.update(|cx| {
            if let Some(github) = github {
                with(cx, |u| u.github = Some(github));
                record(checked, true, cx);
            }
            if searching {
                landed(begun, cx);
            }
        });
    })
    .detach();
}

/// A claimed issue and the worktree made for it.
struct Claimed {
    /// The project the issue was found in, where `gh` is run.
    repo: PathBuf,
    issue: Issue,
    branch: String,
    /// The worktree the run made, and the project root it is added as.
    dir: PathBuf,
}

/// An issue that was claimed and then could not be started, and where to say
/// so.
struct Unstarted {
    repo: PathBuf,
    number: u64,
    why: String,
}

/// Find an issue, claim it and make its worktree. Blocking.
///
/// `None` when there is nothing to do — or when the claim itself failed, in
/// which case there is nobody to tell but stderr, since an issue the app could
/// not edit is one it cannot comment on either. After the claim every failure is
/// the issue's to hear about.
/// What each project looked at said is written into `checked` — a reason where
/// it could not be looked at, `None` where it could — so the rail can show it.
fn begin_blocking(
    roots: &[PathBuf],
    label: &str,
    checked: &mut Vec<(PathBuf, Option<String>)>,
) -> Option<Result<Claimed, Unstarted>> {
    let (repo, issue) =
        roots
            .iter()
            .find_map(|root| match core::candidate_blocking(root, label) {
                Ok(found) => found.map(|issue| (root.clone(), issue)),
                Err(why) => {
                    checked.push((root.clone(), Some(why)));
                    None
                }
            })?;
    if let Err(why) = core::claim_blocking(&repo, issue.number, label) {
        eprintln!("onehand: could not claim issue #{}: {why}", issue.number);
        return None;
    }
    // Caught here as well as around the whole search: past this point the label
    // is already gone, so a panic has to become a comment on the issue, or the
    // issue is left claimed with nothing saying what happened.
    let made = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let base = core::default_branch_blocking(&repo)?;
        worktree::fetch_blocking(&repo, &base)?;
        let top = worktree::repo_top_blocking(&repo).unwrap_or_else(|| repo.clone());
        let branch = core::free_branch_blocking(&top, &core::branch_for(&issue));
        let dir = worktree::worktree_dir(&top, &branch);
        let dir = worktree::branch_off_blocking(&top, &branch, &dir, &format!("origin/{base}"))?;
        Ok::<_, String>((branch, dir))
    }))
    .unwrap_or_else(|_| Err("onehand panicked while preparing the worktree".to_string()));
    Some(match made {
        Ok((branch, dir)) => Ok(Claimed {
            repo,
            issue,
            branch,
            dir,
        }),
        Err(why) => Err(Unstarted {
            repo,
            number: issue.number,
            why,
        }),
    })
}

/// The claim came back: start the session, or say why not.
fn landed(begun: Option<Result<Claimed, Unstarted>>, cx: &mut App) {
    with(cx, |u| u.claiming = false);
    let unstarted = match begun {
        None => return,
        Some(Err(unstarted)) => unstarted,
        Some(Ok(claimed)) => match start(claimed, cx) {
            Ok(()) => return,
            Err(unstarted) => unstarted,
        },
    };
    let said = core::report(&Ending::Failed(unstarted.why), &Ok(None), "");
    cx.background_executor()
        .spawn(async move { tell_issue(&unstarted.repo, unstarted.number, &said) })
        .detach();
}

/// Mint the run's session in the window holding its project, and start
/// watching it.
fn start(claimed: Claimed, cx: &mut App) -> Result<(), Unstarted> {
    let (agent, timeout) = match with(cx, |u| (u.agent.clone(), u.timeout)) {
        Some(settings) => settings,
        None => return Err(unstarted(claimed, "unattended runs are off")),
    };
    let Some(spec) = spec_for(agent.as_deref(), cx) else {
        return Err(unstarted(claimed, "no agent is configured"));
    };
    let Some((window, shell)) = Shared::global(cx)
        .windows
        .iter()
        .find(|w| {
            w.shell
                .upgrade()
                .is_some_and(|s| s.read(cx).holds_root(&claimed.repo))
        })
        .map(|w| (w.handle, w.shell.clone()))
    else {
        return Err(unstarted(
            claimed,
            "the project was closed before the run could start",
        ));
    };
    let Some((uid, session)) = shell
        .upgrade()
        .and_then(|s| s.update(cx, |s, cx| s.run_unattended(claimed.dir.clone(), spec, cx)))
    else {
        return Err(unstarted(
            claimed,
            "the worktree is already open as a project",
        ));
    };

    let watch = cx.subscribe(&session, move |session, event: &ChatEvent, cx| {
        on_event(uid, &session, event, cx)
    });
    // A cancel already winding down keeps the ending it was heading for; only
    // a session that went with nothing pending is reported as closed.
    let release = cx.observe_release(&session, move |_, cx| {
        let pending = with(cx, |u| {
            u.run
                .as_ref()
                .filter(|run| run.uid == uid)
                .and_then(|run| run.ending.clone())
        })
        .flatten();
        settle(uid, pending.unwrap_or(Ending::Closed), cx)
    });
    let clock = cx.spawn(async move |cx| {
        cx.background_executor().timer(timeout).await;
        cx.update(|cx| cancel_toward(uid, Ending::TimedOut(timeout), cx));
    });
    with(cx, |u| {
        u.run = Some(Run {
            claimed,
            uid,
            session: session.downgrade(),
            window,
            shell,
            prompted: false,
            ending: None,
            _watch: watch,
            _release: release,
            _clock: clock,
        })
    });
    // The rail marks the project a run is working on; nothing it watches
    // changed, so it has to be told.
    cx.refresh_windows();
    Ok(())
}

/// `claimed` could not be started, for `why`.
fn unstarted(claimed: Claimed, why: &str) -> Unstarted {
    Unstarted {
        repo: claimed.repo,
        number: claimed.issue.number,
        why: why.to_string(),
    }
}

/// The agent a run starts, with every build it makes pointed at one shared
/// directory.
///
/// Through `env` rather than a new field threaded down to the process spawn,
/// because the adapter is the one process a run gets to set anything on and
/// everything the agent runs inherits from it.
// ponytail: `env` is POSIX; a Windows build would need the variable threaded
// through the ACP spawn instead.
fn spec_for(agent: Option<&str>, cx: &App) -> Option<AgentSpec> {
    let agents = &Shared::global(cx).agents;
    let base = agent
        .and_then(|name| agents.iter().find(|spec| spec.name == name))
        .or_else(|| agents.first())?
        .clone();
    let Some(target) = core::target_dir() else {
        return Some(base);
    };
    let mut args = vec![
        format!("CARGO_TARGET_DIR={}", target.display()),
        base.command,
    ];
    args.extend(base.args);
    Some(AgentSpec {
        name: base.name,
        command: "env".to_string(),
        args,
    })
}

/// One event from the run's session.
fn on_event(uid: u64, session: &Entity<ChatSession>, event: &ChatEvent, cx: &mut App) {
    let Some((prompted, cancelling, shell)) = with(cx, |u| {
        u.run
            .as_ref()
            .filter(|run| run.uid == uid)
            .map(|run| (run.prompted, run.ending.is_some(), run.shell.clone()))
    })
    .flatten() else {
        return;
    };
    match event {
        ChatEvent::Appended if !prompted => prompt(uid, session, cx),
        ChatEvent::Appended => {
            if prompted_by_someone_else(session, true, cx) {
                settle(uid, Ending::TakenOver, cx);
            }
        }
        ChatEvent::TurnEnded if cancelling => settle_pending(uid, cx),
        ChatEvent::TurnEnded => {
            let tail = shell
                .upgrade()
                .and_then(|s| s.read(cx).answer_tail(uid, cx));
            settle(uid, Ending::TurnEnded { tail }, cx);
        }
        ChatEvent::AwaitingUser(ask) => {
            // **Somebody already looking is the person the card asks.** The run
            // never answers a card; handing it over is not answering it.
            if shell.upgrade().is_some_and(|s| s.read(cx).reading(uid, cx)) {
                settle(uid, Ending::TakenOver, cx);
            } else {
                let question = question(*ask, session, cx);
                cancel_toward(uid, Ending::Asked(question), cx);
            }
        }
        ChatEvent::Disconnected if cancelling => settle_pending(uid, cx),
        ChatEvent::Disconnected => settle(uid, Ending::LinkLost, cx),
        ChatEvent::OpenFile(_) => {}
    }
}

/// Send the run's one prompt, once the adapter is up.
///
/// Not before: setting the mode and submitting both need the request channel
/// the handshake installs. The modes arrive ahead of that same event, so by
/// the time the link is up the mode can be checked against what is offered.
fn prompt(uid: u64, session: &Entity<ChatSession>, cx: &mut App) {
    if session.read(cx).chat.link != onehand_core::chat::Link::Connected {
        return;
    }
    // Somebody got there first: a prompt typed between the adapter coming up
    // and this one going out makes the session theirs, and the run's own
    // prompt would either be refused as busy or land on top of their work.
    if session.read(cx).chat.busy || prompted_by_someone_else(session, false, cx) {
        settle(uid, Ending::TakenOver, cx);
        return;
    }
    let Some((mode, text)) = with(cx, |u| {
        let run = u.run.as_mut()?;
        run.prompted = true;
        Some((
            u.mode.clone(),
            core::prompt_for(&run.claimed.issue, &run.claimed.branch),
        ))
    })
    .flatten() else {
        return;
    };
    let offered: Vec<String> = session
        .read(cx)
        .chat
        .modes
        .iter()
        .map(|m| m.id.clone())
        .collect();
    if !offered.contains(&mode) {
        // Every run would fail this way, each on a fresh issue, so the tick
        // stops here rather than working through the backlog to say it.
        let why = format!(
            "the agent offers no mode `{mode}` (it offers: {}). Unattended runs are paused \
             until unattended.mode is fixed and onehand restarted.",
            offered.join(", ")
        );
        with(cx, |u| u.blocked = Some(why.clone()));
        eprintln!("onehand: {why}");
        settle(uid, Ending::Failed(why), cx);
        return;
    }
    let sent = session.update(cx, |session, cx| {
        session.chat.set_mode(&mode);
        session.submit(&text, &[], cx)
    });
    if !sent {
        settle(
            uid,
            Ending::Failed("the agent did not accept the prompt".to_string()),
            cx,
        );
    }
}

/// Whether somebody other than the run has put a prompt into the session,
/// given whether the run has sent its own one yet.
///
/// Any other prompt sent, or one waiting behind the turn, came from the
/// composer or the remote bridge — and either way a person is driving. Counted
/// from what was *sent*, not from the user rows in the transcript: an adapter
/// delivers user chunks of its own mid-turn, and reading those as prompts took
/// runs over that nobody had touched.
fn prompted_by_someone_else(session: &Entity<ChatSession>, run_prompted: bool, cx: &App) -> bool {
    let chat = &session.read(cx).chat;
    chat.queued.is_some() || chat.prompts_sent > usize::from(run_prompted)
}

/// What the parked card asks, in its own words.
fn question(ask: UserAsk, session: &Entity<ChatSession>, cx: &App) -> String {
    let chat = &session.read(cx).chat;
    let asked = match ask {
        UserAsk::Permission => chat
            .pending_permissions()
            .last()
            .map(|(_, p)| p.req.title.clone()),
        UserAsk::Question => chat
            .pending_asks()
            .last()
            .map(|(_, a)| a.req.message.clone()),
    };
    asked.unwrap_or_else(|| ask.headline("The agent"))
}

/// Cancel the run's turn and end as `ending` once it has wound down.
///
/// A session that is already gone — its window closed under it — has nothing
/// to wind down, and is settled on the spot.
fn cancel_toward(uid: u64, ending: Ending, cx: &mut App) {
    let Some(session) = with(cx, |u| {
        let run = u
            .run
            .as_mut()
            .filter(|run| run.uid == uid && run.ending.is_none())?;
        run.ending = Some(ending.clone());
        Some(run.session.clone())
    })
    .flatten() else {
        return;
    };
    let Some(session) = session.upgrade() else {
        settle(uid, ending, cx);
        return;
    };
    session.update(cx, |session, cx| {
        session.chat.cancel_turn();
        cx.notify();
    });
    let wind_down = cx.spawn(async move |cx| {
        cx.background_executor().timer(WIND_DOWN).await;
        cx.update(|cx| settle_pending(uid, cx));
    });
    with(cx, |u| {
        if let Some(run) = u.run.as_mut().filter(|run| run.uid == uid) {
            run._clock = wind_down;
        }
    });
}

/// Settle as whatever the cancel was heading toward.
fn settle_pending(uid: u64, cx: &mut App) {
    let pending = with(cx, |u| {
        u.run
            .as_ref()
            .filter(|run| run.uid == uid)
            .and_then(|run| run.ending.clone())
    })
    .flatten();
    if let Some(ending) = pending {
        settle(uid, ending, cx);
    }
}

/// End the run: close or keep its session, then tell the issue.
///
/// **Deferred**, because this is reached from inside the session's own event
/// and closing the session drops the subscription that event is being
/// delivered through.
fn settle(uid: u64, ending: Ending, cx: &mut App) {
    cx.defer(move |cx| {
        let Some(run) = with(cx, |u| u.run.take_if(|run| run.uid == uid)).flatten() else {
            return;
        };
        cx.refresh_windows();
        let Run {
            claimed,
            window,
            shell,
            ..
        } = run;
        if let Some(shell) = shell.upgrade() {
            let _ = window.update(cx, |_, window, cx| {
                shell.update(cx, |shell, cx| match ending {
                    Ending::TakenOver => shell.adopt_unattended(&claimed.dir, window, cx),
                    _ => shell.end_unattended(&claimed.dir, window, cx),
                })
            });
        }
        cx.background_executor()
            .spawn(async move {
                let pr = if ending.may_have_pr() {
                    core::pr_for_blocking(&claimed.repo, &claimed.branch)
                } else {
                    Ok(None)
                };
                let said = core::report(&ending, &pr, &claimed.branch);
                tell_issue(&claimed.repo, claimed.issue.number, &said);
            })
            .detach();
    });
}

/// Leave `body` on issue `number`, saying on stderr if that failed — there is
/// nowhere else left to say it.
fn tell_issue(repo: &std::path::Path, number: u64, body: &str) {
    if let Err(why) = core::comment_blocking(repo, number, body) {
        eprintln!("onehand: could not comment on issue #{number}: {why}");
    }
}
