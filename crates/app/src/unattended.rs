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
use onehand_core::unattended::{self as core, Ending, Issue};
use onehand_core::worktree;
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
    /// The configuration cannot work (the agent offers no such mode), and every
    /// run would fail the same way on a fresh issue. Said once and stopped.
    halted: bool,
    run: Option<Run>,
    _tick: Task<()>,
}

/// One issue being worked.
struct Run {
    issue: Issue,
    /// The project the issue was found in, where `gh` is run.
    repo: PathBuf,
    /// The worktree the run made, and the project root it was added as.
    dir: PathBuf,
    branch: String,
    uid: u64,
    session: WeakEntity<ChatSession>,
    window: gpui::AnyWindowHandle,
    shell: WeakEntity<crate::shell::Shell>,
    prompted: bool,
    /// The turn has been cancelled, and this is what the run ends as once it
    /// has wound down.
    ending: Option<Ending>,
    _watch: Subscription,
    /// The run's timeout, and after a cancel the wind-down in its place.
    _clock: Task<()>,
}

/// Start the tick if `cfg` asks for one.
///
/// Like the remote bridge, everything here fails by not starting, out loud on
/// stderr: a half-configured feature is the ordinary case, and one that looks
/// on but never runs is indistinguishable from one that is broken.
pub fn boot(cfg: &UnattendedConfig, cx: &mut App) {
    if !cfg.enabled {
        return;
    }
    if cfg.label.trim().is_empty() {
        eprintln!(
            "onehand: unattended runs are enabled but no label is set, so they would \
             pick nothing. Set unattended.label."
        );
        return;
    }
    let (Some(every), Some(timeout)) = (
        core::parse_every(&cfg.every),
        core::parse_every(&cfg.timeout),
    ) else {
        eprintln!(
            "onehand: unattended.every and unattended.timeout take a number and a unit \
             (\"30m\", \"2h\", \"90s\"); got {:?} and {:?}.",
            cfg.every, cfg.timeout
        );
        return;
    };
    let tick = cx.spawn(async move |cx| {
        loop {
            cx.background_executor().timer(every).await;
            cx.update(tick);
        }
    });
    cx.update_global::<Shared, _>(|shared, _| {
        shared.unattended = Some(Unattended {
            label: cfg.label.trim().to_string(),
            timeout,
            mode: cfg.mode.clone(),
            agent: cfg.agent.clone(),
            claiming: false,
            halted: false,
            run: None,
            _tick: tick,
        });
    });
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

/// Look for an issue, if nothing is running.
fn tick(cx: &mut App) {
    let Some(label) = with(cx, |u| {
        let idle = !u.claiming && !u.halted && u.run.is_none();
        idle.then(|| {
            u.claiming = true;
            u.label.clone()
        })
    })
    .flatten() else {
        return;
    };
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
    cx.spawn(async move |cx| {
        let begun = cx
            .background_executor()
            .spawn(async move { begin_blocking(&roots, &label) })
            .await;
        cx.update(|cx| landed(begun, cx));
    })
    .detach();
}

/// A claimed issue and the worktree made for it.
struct Claimed {
    repo: PathBuf,
    issue: Issue,
    branch: String,
    dir: PathBuf,
}

/// Find an issue, claim it and make its worktree. Blocking.
///
/// `None` when there is nothing to do — or when the claim itself failed, in
/// which case there is nobody to tell but stderr, since an issue the app could
/// not edit is one it cannot comment on either. After the claim every failure is
/// the issue's to hear about.
fn begin_blocking(
    roots: &[PathBuf],
    label: &str,
) -> Option<Result<Claimed, (PathBuf, Issue, String)>> {
    let (repo, issue) =
        roots
            .iter()
            .find_map(|root| match core::candidate_blocking(root, label) {
                Ok(found) => found.map(|issue| (root.clone(), issue)),
                Err(why) => {
                    eprintln!("onehand: looking for issues in {}: {why}", root.display());
                    None
                }
            })?;
    if let Err(why) = core::claim_blocking(&repo, issue.number, label) {
        eprintln!("onehand: could not claim issue #{}: {why}", issue.number);
        return None;
    }
    let made = (|| {
        let base = core::default_branch_blocking(&repo)?;
        worktree::fetch_blocking(&repo, &base)?;
        let top = worktree::repo_top_blocking(&repo).unwrap_or_else(|| repo.clone());
        let branch = core::free_branch_blocking(&top, &core::branch_for(&issue));
        let dir = worktree::worktree_dir(&top, &branch);
        let dir = worktree::branch_off_blocking(&top, &branch, &dir, &format!("origin/{base}"))?;
        Ok::<_, String>((branch, dir))
    })();
    Some(match made {
        Ok((branch, dir)) => Ok(Claimed {
            repo,
            issue,
            branch,
            dir,
        }),
        Err(why) => Err((repo, issue, why)),
    })
}

/// The claim came back: start the session, or say why not.
fn landed(begun: Option<Result<Claimed, (PathBuf, Issue, String)>>, cx: &mut App) {
    with(cx, |u| u.claiming = false);
    let claimed = match begun {
        None => return,
        Some(Err((repo, issue, why))) => {
            comment(
                repo,
                issue.number,
                core::report(&Ending::Failed(why), None, ""),
                cx,
            );
            return;
        }
        Some(Ok(claimed)) => claimed,
    };
    if let Err(why) = start(&claimed, cx) {
        comment(
            claimed.repo,
            claimed.issue.number,
            core::report(&Ending::Failed(why), None, &claimed.branch),
            cx,
        );
    }
}

/// Mint the run's session in the window holding its project, and start
/// watching it.
fn start(claimed: &Claimed, cx: &mut App) -> Result<(), String> {
    let (agent, timeout) =
        with(cx, |u| (u.agent.clone(), u.timeout)).ok_or("unattended runs are off")?;
    let spec = spec_for(agent.as_deref(), cx).ok_or("no agent is configured")?;
    let (window, shell) = Shared::global(cx)
        .windows
        .iter()
        .find(|w| {
            w.shell
                .upgrade()
                .is_some_and(|s| s.read(cx).holds_root(&claimed.repo))
        })
        .map(|w| (w.handle, w.shell.clone()))
        .ok_or("the project was closed before the run could start")?;
    let (uid, session) = shell
        .upgrade()
        .and_then(|s| s.update(cx, |s, cx| s.run_unattended(claimed.dir.clone(), spec, cx)))
        .ok_or("the worktree is already open as a project")?;

    let watch = cx.subscribe(&session, move |session, event: &ChatEvent, cx| {
        on_event(uid, &session, event, cx)
    });
    let clock = cx.spawn(async move |cx| {
        cx.background_executor().timer(timeout).await;
        cx.update(|cx| cancel_toward(uid, Ending::TimedOut(timeout), cx));
    });
    with(cx, |u| {
        u.run = Some(Run {
            issue: claimed.issue.clone(),
            repo: claimed.repo.clone(),
            dir: claimed.dir.clone(),
            branch: claimed.branch.clone(),
            uid,
            session: session.downgrade(),
            window,
            shell,
            prompted: false,
            ending: None,
            _watch: watch,
            _clock: clock,
        })
    });
    Ok(())
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
            if taken_over(session, cx) {
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
    let Some((mode, text)) = with(cx, |u| {
        let run = u.run.as_mut()?;
        run.prompted = true;
        Some((u.mode.clone(), core::prompt_for(&run.issue, &run.branch)))
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
        with(cx, |u| u.halted = true);
        let why = format!(
            "the agent offers no mode `{mode}` (it offers: {}). Unattended runs are paused \
             until unattended.mode is fixed and onehand restarted.",
            offered.join(", ")
        );
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

/// Whether somebody other than the run has put a prompt into the session.
///
/// The run sends exactly one. A second, or one waiting behind the turn, came
/// from the composer or the remote bridge — and either way a person is driving.
fn taken_over(session: &Entity<ChatSession>, cx: &App) -> bool {
    let chat = &session.read(cx).chat;
    chat.queued.is_some()
        || chat
            .items
            .iter()
            .filter(|item| matches!(item, onehand_core::chat::ChatItem::User(_)))
            .nth(1)
            .is_some()
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
        let (dir, window) = (run.dir.clone(), run.window);
        if let Some(shell) = run.shell.upgrade() {
            let _ = window.update(cx, |_, window, cx| {
                shell.update(cx, |shell, cx| match ending {
                    Ending::TakenOver => shell.adopt_unattended(&dir, window, cx),
                    _ => shell.end_unattended(&dir, window, cx),
                })
            });
        }
        let Run {
            repo,
            issue,
            branch,
            ..
        } = run;
        cx.background_executor()
            .spawn(async move {
                let pr = if ending.may_have_pr() {
                    core::pr_for_blocking(&repo, &branch).ok().flatten()
                } else {
                    None
                };
                let said = core::report(&ending, pr.as_deref(), &branch);
                if let Err(why) = core::comment_blocking(&repo, issue.number, &said) {
                    eprintln!(
                        "onehand: could not comment on issue #{}: {why}",
                        issue.number
                    );
                }
            })
            .detach();
    });
}

/// Leave `body` on issue `number`, off the UI loop.
fn comment(repo: PathBuf, number: u64, body: String, cx: &mut App) {
    cx.background_executor()
        .spawn(async move {
            if let Err(why) = core::comment_blocking(&repo, number, &body) {
                eprintln!("onehand: could not comment on issue #{number}: {why}");
            }
        })
        .detach();
}
