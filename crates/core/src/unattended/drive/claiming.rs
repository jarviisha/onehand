//! The tick, a search asked for now and a pick: from what stops a search to
//! the task a claim is kept as.

use super::{
    earlier, issue_task, tell, Effect, Look, Looked, On, Pick, Search, Signal, UnattendedDrive,
    World,
};
use crate::task::tasks::link::Seen;
use crate::unattended::{self as core, Claimed, Skip, Taking, Unstarted, Work};
use crate::workflow::Setup;
use crate::Now;

impl UnattendedDrive {
    /// Look at every switched-on project (`roots`), and search them for an
    /// issue if a slot is free and nothing stops every run. `asked_from` is
    /// the window a person asked from, to be told what came of it.
    ///
    /// **Every tick looks, even while a run is live**: the rows show what the
    /// last look found, and a look only before a search left them as old as
    /// the run was long. **A report not yet delivered** is tried again first,
    /// switched on or not, since an issue picked by hand needs no switch.
    /// **Nothing switched on asks no connector anything**, so the feature
    /// costs nobody who has not switched a project on.
    pub fn tick<W: Copy + PartialEq>(
        &mut self,
        asked_from: Option<W>,
        seen: &Seen<W>,
        roots: Vec<On>,
        world: &World,
    ) -> Vec<Effect<W>> {
        let mut fx = self.deliver_all(seen);
        if roots.is_empty() {
            return fx;
        }
        let stopped = self.at_cap(seen).or_else(|| self.cannot_start(world));
        // A project with no check command is still searched, for the issues
        // whose workflow runs none; its row says what the default lacks.
        let lacking = roots
            .iter()
            .filter_map(|(p, has)| {
                Some((
                    p.root.clone(),
                    self.lacks_check(*has, &self.workflow, world)?,
                ))
            })
            .collect();
        let idle = !self.claiming && self.blocked.is_none() && stopped.is_none();
        let search = idle.then(|| Search {
            label: self.label.clone(),
            choosing: self.choosing(world),
            judging: self.judging(seen, world),
            earlier: earlier(seen),
        });
        self.claiming |= search.is_some();
        fx.push(Effect::Look(Box::new(Look {
            connectors: self.connectors,
            roots,
            lacking,
            search,
            asked_from,
        })));
        fx
    }

    /// Look for a labelled issue now, rather than at the next tick, saying in
    /// `window` what came of it: every way it can do nothing is said rather
    /// than left to look like nothing happened.
    pub fn look_now<W: Copy + PartialEq>(
        &mut self,
        window: W,
        seen: &Seen<W>,
        roots: Vec<On>,
        world: &World,
    ) -> Vec<Effect<W>> {
        let why = match roots.is_empty() {
            true => Some("No project is switched on for unattended runs.".to_string()),
            false => self.at_cap(seen).or_else(|| match self.claiming {
                true => Some("A run is already starting.".to_string()),
                false => self
                    .blocked
                    .clone()
                    .or_else(|| self.cannot_start(world))
                    .map(|why| format!("Nothing can be picked up: {why}")),
            }),
        };
        match why {
            Some(said) => vec![Effect::Signal(Signal::Warn { window, said })],
            None => {
                let mut fx = self.tick(Some(window), seen, roots, world);
                // Its button waits while the look claims what it found.
                fx.push(Effect::Refresh);
                fx
            }
        }
    }

    /// Work `pick` now, picked by hand in `window`, or why not.
    ///
    /// **Refused while the cap is reached**, the rule for picked and found
    /// alike, and **before the claim** for whatever would fail the run: the
    /// issue would otherwise be claimed and commented on for nothing.
    pub fn pick<W: Copy>(
        &mut self,
        pick: Pick,
        window: W,
        seen: &Seen<W>,
        world: &World,
    ) -> Result<Vec<Effect<W>>, String> {
        if let Some(why) = self.at_cap(seen) {
            return Err(why);
        }
        if self.claiming {
            return Err("An unattended run is starting — one at a time.".to_string());
        }
        if let Some(why) = self.cannot_start(world) {
            return Err(format!("Nothing can be started: {why}"));
        }
        // Said apart from what stops every run: this is the one workflow
        // picked, on this one project, and another pick may well start.
        if let Some(why) = self.lacks_check(pick.has_check, &pick.workflow, world) {
            return Err(format!("Cannot start on this project: {why}"));
        }
        self.claiming = true;
        Ok(vec![Effect::Take {
            pick: Box::new(pick),
            window,
            earlier: earlier(seen),
            connectors: self.connectors,
        }])
    }

    /// The look landed: file what it found, and claim what the search found
    /// if no task took the issue up meanwhile.
    pub(super) fn looked<W: Copy>(
        &mut self,
        looked: Looked<W>,
        seen: &Seen<W>,
        roots: &Vec<On>,
        now: Now,
    ) -> Vec<Effect<W>> {
        let Looked {
            asked_from,
            searching,
            answer,
        } = looked;
        let mut fx = Vec::new();
        // A look that panicked is said, and treated as nothing found.
        let (found, held) = match answer {
            Some(answer) => {
                self.file_accounts(answer.accounts, now);
                self.record(answer.checked, Some(roots));
                fx.push(Effect::Refresh);
                (answer.found, answer.held)
            }
            None => (None, None),
        };
        // Checked again here, where tasks start: a Retry pressed while the
        // search ran would otherwise have the issue claimed under it.
        let (found, held) = match found {
            Some(found) if !found.still_free(&earlier(seen)) => (
                None,
                Some(
                    "Nothing was picked up: the issue found is being worked, and keeps its label"
                        .to_string(),
                ),
            ),
            found => (found, held),
        };
        match found {
            Some(found) => fx.push(Effect::Claim {
                found: Box::new(found),
                label: self.label.clone(),
                asked_from,
                held,
            }),
            None if searching => fx.extend(self.unclaimed(asked_from, held)),
            None => {}
        }
        fx
    }

    /// Nothing was claimed: a search somebody asked for says so, with why
    /// when something held it; one the tick ran says nothing.
    fn unclaimed<W>(&mut self, asked_from: Option<W>, held: Option<String>) -> Vec<Effect<W>> {
        self.claiming = false;
        let mut fx = vec![Effect::Refresh];
        if let Some(window) = asked_from {
            let said = held.unwrap_or_else(|| {
                format!(
                    "No open issue of yours labelled `{}` in the projects switched on.",
                    self.label
                )
            });
            fx.push(Effect::Signal(Signal::Warn { window, said }));
        }
        fx
    }

    /// A claim came back: start its task, or tell the issue why not.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn landed<W: Copy + PartialEq>(
        &mut self,
        begun: Option<Result<Claimed, Unstarted>>,
        asked_from: Option<W>,
        picked_in: Option<W>,
        held: Option<String>,
        holding: &[(W, Option<String>)],
        new_task: String,
        seen: &Seen<W>,
        world: &World,
    ) -> Vec<Effect<W>> {
        let unstarted = match begun {
            None => return self.unclaimed(asked_from, held),
            Some(Err(unstarted)) => unstarted,
            Some(Ok(claimed)) => {
                match self.start(claimed, picked_in, holding, new_task, seen, world) {
                    // One claim per search, so a slot still free after it is
                    // filled by looking again now rather than at the next
                    // tick. The look stops itself at the cap.
                    Ok(mut fx) => {
                        self.claiming = false;
                        fx.insert(0, Effect::Refresh);
                        fx.push(Effect::Tick);
                        return fx;
                    }
                    Err(unstarted) => unstarted,
                }
            }
        };
        self.claiming = false;
        vec![Effect::Refresh, tell(unstarted)]
    }

    /// Keep a claimed issue as a task of its workflow, then ask for its place
    /// in the window holding its project: the one it was picked in when that
    /// one does, so its session comes up in front of the person who asked.
    fn start<W: Copy + PartialEq>(
        &mut self,
        claimed: Claimed,
        picked_in: Option<W>,
        holding: &[(W, Option<String>)],
        new_task: String,
        seen: &Seen<W>,
        world: &World,
    ) -> Result<Vec<Effect<W>>, Unstarted> {
        let Some((window, check)) = holding
            .iter()
            .find(|(window, _)| Some(*window) == picked_in)
            .or(holding.first())
            .cloned()
        else {
            return Err(Unstarted::of(
                claimed,
                "the project was closed before the run could start",
            ));
        };
        let (branch, base, dir) = match claimed.work.clone() {
            Work::Cut { branch, base, dir } => (branch, base, dir),
            // A review is answered on the task's own snapshot, as any retry
            // is: the configured workflow is for new tasks only.
            Work::Review { task, note } => {
                return self.answer_review(claimed, task, new_task, note, window, seen);
            }
        };
        let template = match self.workflow(&claimed.workflow, world) {
            Ok(template) => template,
            Err(why) => return Err(Unstarted::of(claimed, &why)),
        };
        if let Some(why) = core::check_missing(&template, check.is_some()) {
            return Err(Unstarted::of(claimed, &why));
        }
        let setup = Setup {
            repo: claimed.repo.clone(),
            dir,
            branch: Some(branch),
            agent: self.run_agent(&world.agents),
            check,
            // Left empty, the agent stays in the mode it starts in, which
            // asks more rather than less.
            mode: crate::config::mode_set(&self.run_mode(&world.agents)),
            forge: claimed.forge.map(|forge| forge.name().to_string()),
        };
        let task = issue_task(
            new_task,
            claimed,
            template,
            setup,
            base,
            picked_in.is_some(),
        );
        // Counted against the cap until it has asked for its place.
        self.starting.insert(task.id.clone());
        Ok(vec![Effect::Submit {
            task: Box::new(task),
            window,
        }])
    }

    /// Answer the review on the pull request of issue task `task` with a new
    /// run `run`, telling it `note`, from the step its status checks send
    /// back to.
    fn answer_review<W: Copy>(
        &mut self,
        claimed: Claimed,
        task: String,
        run: String,
        note: String,
        window: W,
        seen: &Seen<W>,
    ) -> Result<Vec<Effect<W>>, Unstarted> {
        let Some(last) = seen.task(&task).and_then(|t| t.runs.last()) else {
            return Err(Unstarted::of(
                claimed,
                "the task of its pull request is gone",
            ));
        };
        if seen.busy(&task) {
            return Err(Unstarted::of(claimed, "a run is still working on it"));
        }
        let from = last.template.repair_step().map(str::to_string);
        self.starting.insert(task.clone());
        Ok(vec![Effect::AnswerReview {
            task,
            run,
            template: Box::new(last.template.clone()),
            from,
            note,
            window,
        }])
    }

    /// What claiming a picked issue would come to landed: claim it, if no
    /// task took it up meanwhile, or say in `window` why not.
    pub(super) fn taken<W: Copy>(
        &mut self,
        pick: Pick,
        window: W,
        taken: Result<(Option<&'static dyn crate::connector::Connector>, Taking), String>,
        seen: &Seen<W>,
    ) -> Vec<Effect<W>> {
        let named = pick.tracker.named(&pick.row.issue);
        // Checked again here, where tasks start: a Retry pressed while the
        // pull request was looked up would otherwise have the issue claimed
        // under it.
        let free = core::still_free(&earlier(seen), &pick.tracker, pick.row.issue.number);
        match taken.and_then(|t| {
            if free {
                Ok(t)
            } else {
                Err(Skip::Busy.said(&named))
            }
        }) {
            Ok((forge, taking)) => vec![Effect::ClaimPicked {
                pick: Box::new(pick),
                window,
                forge,
                taking: Box::new(taking),
                label: self.label.clone(),
            }],
            Err(why) => self.refused_pick(window, why),
        }
    }

    /// A picked issue's claim landed: start it, or say why not where it was
    /// picked, and on the issue once it was claimed.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn pick_claimed<W: Copy + PartialEq>(
        &mut self,
        window: W,
        named: &str,
        begun: Result<Result<Claimed, Unstarted>, String>,
        holding: &[(W, Option<String>)],
        new_task: String,
        seen: &Seen<W>,
        world: &World,
    ) -> Vec<Effect<W>> {
        // Nothing was claimed, so the issue has nothing to be told.
        let begun = match begun {
            Err(why) => return self.refused_pick(window, why),
            Ok(begun) => begun,
        };
        let mut fx = Vec::new();
        if let Err(Unstarted { why, .. }) = &begun {
            fx.push(Effect::Signal(Signal::Warn {
                window,
                said: format!("Could not start on issue {named}: {why}"),
            }));
        }
        let picked_in = begun.is_ok().then_some(window);
        fx.extend(self.landed(
            Some(begun),
            None,
            picked_in,
            None,
            holding,
            new_task,
            seen,
            world,
        ));
        fx
    }

    fn refused_pick<W>(&mut self, window: W, why: String) -> Vec<Effect<W>> {
        self.claiming = false;
        vec![
            Effect::Refresh,
            Effect::Signal(Signal::Warn { window, said: why }),
        ]
    }
}
