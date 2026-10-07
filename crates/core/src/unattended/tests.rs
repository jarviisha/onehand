use super::report::ended;
use super::*;
use crate::connector::fake::Fake;
use crate::connector::PrState;
use crate::workflow::{Outcome, Stop};

/// The test forge, as the tracker an issue on it lives in.
fn forge() -> Tracker {
    Tracker::Forge(&Fake::SERVING)
}

/// A tracker over a scratch issue file of its own, holding `issues`.
fn local(name: &str, issues: &[(&str, &[&str])]) -> (Tracker, PathBuf) {
    let dir = std::env::temp_dir().join(format!("onehand-local-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let file = dir.join("issues.json");
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
    (Tracker::Local(file), dir)
}

/// The issue a search would take first.
fn candidate_blocking(
    tracker: &Tracker,
    root: &Path,
    label: &str,
) -> Result<Option<Issue>, String> {
    Ok(candidates_blocking(tracker, root, label)?
        .into_iter()
        .next()
        .map(|row| row.issue))
}

fn issue(number: u64, title: &str) -> Issue {
    Issue::new(number, title.to_string(), String::new())
}

#[test]
fn an_interval_is_a_number_and_a_unit() {
    assert_eq!(parse_every("30m"), Some(Duration::from_secs(1800)));
    assert_eq!(parse_every("2h"), Some(Duration::from_secs(7200)));
    assert_eq!(parse_every(" 90s "), Some(Duration::from_secs(90)));
    assert_eq!(parse_every("soon"), None);
    assert_eq!(parse_every("0m"), None);
    assert_eq!(parse_every("m"), None);
    assert_eq!(parse_every(""), None);
}

#[test]
fn the_claim_stays_true_if_nothing_follows_it() {
    let said = claim_comment("auto", None);
    assert!(said.contains("started"));
    assert!(said.contains("re-add `auto`"));
    assert!(!said.contains("is working"));
    let answering = claim_comment("auto", Some("https://x/pull/2"));
    assert!(
        answering.contains("review on https://x/pull/2"),
        "{answering}"
    );
}

#[test]
fn a_picked_claim_says_how_to_retry_without_a_label() {
    let said = picked_claim_comment(None);
    assert!(picked_claim_comment(Some("https://x/pull/2")).contains("review on https://x/pull/2"));
    assert!(said.contains("started") && said.contains("picked by hand"));
    assert!(
        !said.contains("re-add"),
        "a picked issue may never have had the label"
    );
}

#[test]
fn an_empty_label_picks_nothing_without_asking() {
    let nowhere = std::env::temp_dir();
    assert_eq!(candidate_blocking(&forge(), &nowhere, ""), Ok(None));
    assert_eq!(candidate_blocking(&forge(), &nowhere, "   "), Ok(None));
}

#[test]
fn the_oldest_issue_is_taken_first() {
    let found = candidate_blocking(&forge(), &std::env::temp_dir(), "auto").unwrap();
    assert_eq!(found.map(|i| i.number), Some(4));
    static NONE_LABELLED: Fake = Fake {
        labelled: &[],
        ..Fake::SERVING
    };
    assert_eq!(
        candidate_blocking(
            &Tracker::Forge(&NONE_LABELLED),
            &std::env::temp_dir(),
            "auto"
        ),
        Ok(None)
    );
}

#[test]
fn a_found_issue_keeps_every_label_it_carries() {
    let (tracker, _dir) = local("labels", &[("first", &["auto", "bug"])]);
    let found = candidates_blocking(&tracker, &std::env::temp_dir(), "auto").unwrap();
    assert_eq!(found[0].labels, ["auto", "bug"]);
}

#[test]
fn a_workflow_label_chooses_the_workflow_in_the_table_order() {
    let by_label: std::collections::BTreeMap<String, String> = [
        ("bug".to_string(), "fix".to_string()),
        ("docs".to_string(), "write".to_string()),
    ]
    .into();
    let labels = |l: &[&str]| l.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let pick = |l: &[&str]| workflow_for(&labels(l), &by_label, "d", "auto").to_string();
    assert_eq!(pick(&["auto"]), "d");
    assert_eq!(pick(&["auto", "docs"]), "write");
    assert_eq!(
        pick(&["docs", "bug"]),
        "fix",
        "the table's order, not the issue's"
    );
    assert_eq!(workflow_for(&[], &Default::default(), "d", "auto"), "d");
}

#[test]
fn the_trigger_label_never_chooses_a_workflow() {
    let by_label: std::collections::BTreeMap<String, String> =
        [("auto".to_string(), "x".to_string())].into();
    let labels = ["auto".to_string()];
    assert_eq!(workflow_for(&labels, &by_label, "d", "auto"), "d");
    assert!(workflow_label_refused("auto", "auto", &by_label).is_some());
    assert!(workflow_label_refused("", "auto", &by_label).is_some());
    assert!(workflow_label_refused("bug", "auto", &by_label).is_none());
    let taken: std::collections::BTreeMap<String, String> =
        [("bug".to_string(), "x".to_string())].into();
    assert!(workflow_label_refused("bug", "auto", &taken).is_some());
}

#[test]
fn a_listing_past_its_bound_is_cut_and_says_so() {
    let (rows, cut) = open_issues_blocking(&forge(), &std::env::temp_dir()).unwrap();
    assert_eq!(rows.len(), ISSUES_SHOWN);
    assert!(cut);
    let (rows, cut) = bounded(Vec::new());
    assert!(rows.is_empty() && !cut);
}

#[test]
fn a_local_issue_is_found_by_its_label_and_claimed_in_its_own_file() {
    let (tracker, dir) = local(
        "claim",
        &[
            ("first", &["auto"]),
            ("second", &["auto", "ui"]),
            ("third", &[]),
        ],
    );
    let root = std::env::temp_dir();
    let found = candidate_blocking(&tracker, &root, "auto")
        .unwrap()
        .unwrap();
    assert_eq!(found.number, 1, "the oldest labelled issue goes first");
    claim_blocking(&tracker, &root, 1, "auto", None).unwrap();
    let Tracker::Local(file) = &tracker else {
        unreachable!()
    };
    let kept = crate::issues::load_blocking(file).unwrap();
    let claimed = kept.get(1).unwrap();
    assert!(claimed.labels.is_empty(), "the label is the claim");
    assert!(claimed.notes[0].text.contains("started"));
    assert_eq!(
        candidate_blocking(&tracker, &root, "auto")
            .unwrap()
            .map(|i| i.number),
        Some(2)
    );
    let (rows, cut) = open_issues_blocking(&tracker, &root).unwrap();
    assert_eq!(rows.len(), 3);
    assert!(!cut && rows.iter().all(|row| row.author.is_empty()));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_synced_project_runs_only_what_the_user_wrote_and_claims_it_on_both_sides() {
    use crate::connector::memory::Forge;
    use crate::issues::Snapshot;
    let label = |title: &str| Snapshot {
        title: title.into(),
        body: String::new(),
        open: true,
        labels: vec!["auto".into()],
    };
    // #7 is the user's, #8 somebody else's; both carry the label.
    static FORGE: std::sync::OnceLock<Forge> = std::sync::OnceLock::new();
    let forge = FORGE.get_or_init(|| Forge {
        mine: vec!["7".into()],
        ..Forge::with(vec![(7, label("mine")), (8, label("theirs"))])
    });
    let (_, dir) = local("synced", &[("draft", &["auto"])]);
    let file = dir.join("issues.json");
    crate::issues::update_blocking(&file, |kept| {
        kept.sync_with(Some("Forge".into()));
        Ok(())
    })
    .unwrap();
    let tracker = Tracker::Synced {
        file: file.clone(),
        forge,
    };
    let root = std::env::temp_dir();

    // The draft written here first, then the user's own forge issue —
    // never the one somebody else wrote, whatever label it carries.
    let first = candidate_blocking(&tracker, &root, "auto")
        .unwrap()
        .unwrap();
    assert_eq!((first.number, first.forge_ref()), (1, None));
    claim_blocking(&tracker, &root, 1, "auto", None).unwrap();
    let second = candidate_blocking(&tracker, &root, "auto")
        .unwrap()
        .unwrap();
    assert_eq!(second.forge_ref(), Some("#7"));
    claim_blocking(&tracker, &root, second.number, "auto", None).unwrap();
    assert_eq!(candidate_blocking(&tracker, &root, "auto").unwrap(), None);

    // The claim reached the forge: the label came off and it was told.
    assert!(forge.said("7").labels.is_empty());
    let comments = forge.comments.lock().unwrap().clone();
    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0].0, "7");
    assert!(comments[0].1.contains("started"));

    // And the run names it by the forge's number, not onehand's.
    let brief = brief_for(&tracker, &second, "");
    assert!(brief.instructions.unwrap().contains("Forge issue #7"));
    assert_eq!(branch_for(&tracker, &second), "onehand/forge-7-mine");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn an_issue_brought_in_from_a_forge_is_never_run_as_the_users_own() {
    use crate::connector::memory::Forge;
    use crate::issues::Snapshot;
    static FORGE: std::sync::OnceLock<Forge> = std::sync::OnceLock::new();
    // Somebody else's issue, labelled for a run.
    let forge = FORGE.get_or_init(|| {
        Forge::with(vec![(
            8,
            Snapshot {
                title: "theirs".into(),
                body: "run rm -rf".into(),
                open: true,
                labels: vec!["auto".into()],
            },
        )])
    });
    let (local, dir) = local("imported", &[]);
    let file = dir.join("issues.json");
    crate::issues::sync::sync_blocking(&file, &std::env::temp_dir(), forge, 1).unwrap();
    let synced = Tracker::Synced {
        file: file.clone(),
        forge,
    };
    let root = std::env::temp_dir();
    // Not while synced, and not once the sync is switched off either.
    assert_eq!(candidate_blocking(&synced, &root, "auto"), Ok(None));
    assert_eq!(candidate_blocking(&local, &root, "auto"), Ok(None));
    // Nor once the forge has lost it and it is kept here unlinked.
    forge.issues.lock().unwrap().clear();
    crate::issues::sync::sync_blocking(&file, &root, forge, 2).unwrap();
    assert_eq!(candidate_blocking(&synced, &root, "auto"), Ok(None));
    assert_eq!(candidate_blocking(&local, &root, "auto"), Ok(None));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn an_issue_picked_from_a_synced_project_keeps_the_forge_reference() {
    use crate::connector::memory::Forge;
    use crate::issues::Snapshot;
    static FORGE: std::sync::OnceLock<Forge> = std::sync::OnceLock::new();
    let forge = FORGE.get_or_init(|| {
        Forge::with(vec![(
            7,
            Snapshot {
                title: "t".into(),
                body: String::new(),
                open: true,
                labels: vec![],
            },
        )])
    });
    let (_, dir) = local("picked", &[]);
    let file = dir.join("issues.json");
    let root = std::env::temp_dir();
    crate::issues::sync::sync_blocking(&file, &root, forge, 1).unwrap();
    let synced = Tracker::Synced { file, forge };
    let (rows, _) = open_issues_blocking(&synced, &root).unwrap();
    assert_eq!(rows[0].issue.forge_ref(), Some("#7"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn waiting_on_a_person_spends_none_of_the_budget() {
    let t0 = Instant::now();
    let s = Duration::from_secs;
    let mut budget = Budget::start(s(100), t0);
    assert_eq!(budget.left(t0 + s(30)), s(70));
    budget.pause(t0 + s(30));
    budget.pause(t0 + s(40));
    assert_eq!(budget.left(t0 + s(500)), s(70));
    budget.resume(t0 + s(500));
    budget.resume(t0 + s(510));
    assert_eq!(budget.left(t0 + s(520)), s(50));
    assert_eq!(budget.left(t0 + s(9999)), Duration::ZERO);
}

#[test]
fn an_issue_is_shown_by_its_forge_number_and_a_kept_one_as_a_draft() {
    assert_eq!(forge().shown(&issue(7, "a")), "#7");
    let (kept, dir) = local("shown", &[]);
    assert_eq!(kept.shown(&issue(3, "a")), "Draft");
    // Kept here and in step with the forge: the forge's number, never ours.
    assert_eq!(kept.shown(&issue(3, "a").at("#41".into())), "#41");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn an_issue_is_named_by_its_forge_number_and_a_draft_by_its_title() {
    assert_eq!(forge().named(&issue(7, "a")), "#7");
    let (kept, dir) = local("named", &[]);
    assert_eq!(kept.named(&issue(3, "Fix it")), "\u{201c}Fix it\u{201d}");
    assert_eq!(kept.named(&issue(3, "a").at("#41".into())), "#41");
    let source = IssueSource {
        tracker: kept.to_ref(),
        number: 3,
        forge_ref: None,
        forge: None,
        base: "main".into(),
        picked: false,
        unsent: Vec::new(),
        notes: Vec::new(),
    };
    assert_eq!(source.named("Fix it"), kept.named(&issue(3, "Fix it")));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn every_title_makes_a_valid_branch() {
    let long = "word ".repeat(60);
    for title in [
        "Fix the rail",
        "!!!???",
        "",
        &long,
        "émoji 🚀 ünïcode",
        "a.lock",
    ] {
        let branch = branch_for(&forge(), &issue(7, title));
        assert!(
            crate::worktree::validate_branch(&branch).is_ok(),
            "{title:?} gave {branch:?}"
        );
        assert!(branch.starts_with("onehand/forge-7"));
        assert!(branch.len() <= "onehand/forge-7-".len() + 40);
    }
    assert_eq!(
        branch_for(&forge(), &issue(3, "Fix the rail!")),
        "onehand/forge-3-fix-the-rail"
    );
    assert_eq!(branch_for(&forge(), &issue(3, "!!!")), "onehand/forge-3");
}

#[test]
fn a_kept_issue_and_a_forge_issue_of_one_number_never_share_a_branch() {
    let (kept, dir) = local("branch", &[]);
    let same = issue(3, "Fix the rail");
    let here = branch_for(&kept, &same);
    let there = branch_for(&forge(), &same);
    assert_eq!(here, "onehand/local-3-fix-the-rail");
    assert_ne!(here, there);
    // Kept in step with the forge, it goes by the forge's name for it.
    let synced = Tracker::Synced {
        file: dir.join("issues.json"),
        forge: &Fake::SERVING,
    };
    assert_eq!(
        branch_for(&synced, &issue(3, "Fix the rail").at("#41".into())),
        "onehand/forge-41-fix-the-rail"
    );
    assert_eq!(branch_for(&synced, &same), here);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_brief_keeps_the_body_whole_and_names_the_issue() {
    let body = "Steps:\n\n```rust\nfn main() {}\n```\n\nThat is all.";
    let issue = Issue {
        body: body.to_string(),
        ..issue(42, "Crash on open")
    };
    let brief = brief_for(&forge(), &issue, "");
    assert_eq!(brief.title, "Crash on open");
    assert_eq!(brief.body, body);
    let asked = brief.instructions.unwrap();
    assert!(asked.contains("Forge issue #42"), "{asked}");
    assert!(asked.contains("question"), "{asked}");
    let (kept, dir) = local("brief", &[]);
    let asked = brief_for(&kept, &issue, "").instructions.unwrap();
    assert!(asked.contains("kept in onehand"), "{asked}");
    let _ = std::fs::remove_dir_all(dir);
}

/// A run that ended as `outcome`, after asking its agent something.
fn ran(outcome: Outcome) -> PendingReport {
    PendingReport {
        run: "1".into(),
        outcome: Some(outcome),
        started: true,
        ended_on: None,
        asked: None,
        notes: Vec::new(),
    }
}

#[test]
fn every_outcome_has_a_sentence_and_the_work_leads() {
    let outcomes = [
        Outcome::Done,
        Outcome::Stopped(Stop::ByPerson),
        Outcome::Stopped(Stop::TakenOver),
        Outcome::Stopped(Stop::TimedOut),
        Outcome::Stopped(Stop::LinkLost),
        Outcome::Stopped(Stop::Closed),
        Outcome::Exhausted {
            step: "Plan".into(),
        },
        Outcome::Failed("git refused".into()),
    ];
    for outcome in outcomes {
        let pending = ran(outcome);
        let pr = report(&pending, &Ok(opened(PrState::Open, true)), "b");
        assert!(pr.starts_with("onehand opened https://x/pull/2."), "{pr}");
        let commits = report(&pending, &Ok(Verdict::Commits(2)), "b");
        assert!(
            commits.starts_with("onehand left 2 commits on `b`."),
            "{commits}"
        );
        let unknown = report(&pending, &Err("gh: offline".into()), "b");
        assert!(unknown.contains("could not tell") && unknown.contains("gh: offline"));
        assert!(!unknown.contains("no commit"), "{unknown}");
        assert_ne!(ended(pending.outcome.as_ref()), "");
    }
    let one = report(&ran(Outcome::Done), &Ok(Verdict::Commits(1)), "b");
    assert!(one.starts_with("onehand left 1 commit on `b`."), "{one}");
    let none = report(
        &ran(Outcome::Stopped(Stop::TimedOut)),
        &Ok(Verdict::Commits(0)),
        "b",
    );
    assert!(none.starts_with("onehand left no commit"), "{none}");
    assert!(none.contains("timeout"), "{none}");
}

/// The pull request on the branch, as the forge has it when the report goes.
fn opened(state: PrState, draft: bool) -> Verdict {
    Verdict::PullRequest {
        url: "https://x/pull/2".into(),
        state,
        draft,
    }
}

#[test]
fn the_report_says_what_became_of_the_pull_request() {
    let done = ran(Outcome::Done);
    let said = |verdict| report(&done, &Ok(verdict), "b");
    assert!(said(opened(PrState::Open, false)).contains("ready for review"));
    assert!(!said(opened(PrState::Open, true)).contains("ready for review"));
    assert!(said(opened(PrState::Merged, false)).contains("merged"));
    assert!(said(opened(PrState::Closed, false)).contains("closed"));
}

#[test]
fn what_the_last_step_ended_on_reaches_the_issue_unless_it_got_to_the_end() {
    let tail = Some("Should I use A\nor B?".to_string());
    let missed = PendingReport {
        ended_on: tail.clone(),
        ..ran(Outcome::Exhausted {
            step: "Plan".into(),
        })
    };
    let said = report(&missed, &Ok(Verdict::Commits(0)), "b");
    assert!(said.contains("> Should I use A\n> or B?"), "{said}");
    let done = PendingReport {
        ended_on: tail,
        ..ran(Outcome::Done)
    };
    let said = report(&done, &Ok(Verdict::Commits(1)), "b");
    assert!(!said.contains('>'), "{said}");
}

#[test]
fn a_run_that_ended_on_a_card_says_the_question() {
    let asked = PendingReport {
        asked: Some("Run awk?".into()),
        ..ran(Outcome::Stopped(Stop::LinkLost))
    };
    let said = report(&asked, &Ok(Verdict::Commits(0)), "b");
    assert!(
        said.contains("nobody answered") && said.contains("> Run awk?"),
        "{said}"
    );
}

#[test]
fn a_run_that_failed_before_asking_anything_could_not_start() {
    let refused = PendingReport {
        started: false,
        ..ran(Outcome::Failed("the agent offers no mode `x`".into()))
    };
    let said = report(&refused, &Ok(Verdict::Commits(0)), "b");
    assert_eq!(
        said,
        "onehand could not start the run: the agent offers no mode `x`"
    );
    // Failing later is a failure of the run, with its work said first.
    let later = report(
        &ran(Outcome::Failed("boom".into())),
        &Ok(Verdict::Commits(1)),
        "b",
    );
    assert!(later.starts_with("onehand left 1 commit"), "{later}");
}

#[test]
fn a_run_cut_off_and_let_go_says_so() {
    let let_go = PendingReport {
        outcome: None,
        ..ran(Outcome::Done)
    };
    let said = report(&let_go, &Ok(Verdict::Commits(0)), "b");
    assert!(said.contains("cut off"), "{said}");
}

#[test]
fn every_tracker_kept_by_name_resolves_back() {
    let all: [&'static dyn Connector; 1] = [&Fake::SERVING];
    let file = PathBuf::from("/kept/issues.json");
    let trackers = [
        forge(),
        Tracker::Local(file.clone()),
        Tracker::Synced {
            file,
            forge: &Fake::SERVING,
        },
    ];
    for tracker in trackers {
        // Exhaustive on purpose: a new tracker cannot be added without being
        // kept and found again here.
        match &tracker {
            Tracker::Forge(_) | Tracker::Local(_) | Tracker::Synced { .. } => {}
        }
        let kept = tracker.to_ref();
        assert_eq!(kept.resolve(&all).map(|t| t.to_ref()), Some(kept.clone()));
        let source = IssueSource {
            tracker: kept,
            number: 7,
            forge_ref: None,
            forge: None,
            base: "main".into(),
            picked: false,
            unsent: Vec::new(),
            notes: Vec::new(),
        };
        assert_eq!(source.shown(), tracker.shown(&issue(7, "a")));
    }
    assert!(forge().to_ref().resolve(&[]).is_none());
}

fn holder(task: &str, shown: &str) -> Holder {
    Holder {
        task: task.into(),
        shown: shown.into(),
    }
}

fn slots(holders: Vec<Holder>, starting: usize, at_once: u32) -> Slots {
    Slots {
        holders,
        starting,
        at_once,
        waiting: 0,
        waiting_cap: None,
    }
}

#[test]
fn the_cap_counts_working_runs_only() {
    assert!(slots(vec![], 0, 1).full().is_none());
    assert!(slots(vec![holder("a", "#1")], 0, 1).full().is_some());
    assert!(slots(vec![holder("a", "#1")], 0, 2).full().is_none());
    assert!(slots(vec![], 1, 1).full().is_some());
    assert!(slots(vec![], 0, 0).full().is_some());
}

#[test]
fn the_slots_line_names_each_holder_in_order() {
    assert_eq!(slots(vec![], 0, 2).said(), "Slots: 0 of 2");
    assert_eq!(
        slots(vec![holder("a", "#12 · Work an issue (Implement)")], 0, 1).said(),
        "Slots: 1 of 1 — #12 · Work an issue (Implement)"
    );
    assert_eq!(
        slots(vec![holder("a", "#3"), holder("b", "#9")], 1, 3).said(),
        "Slots: 3 of 3 — #3, #9, 1 starting"
    );
}

#[test]
fn a_full_slot_is_refused_with_the_slots_line() {
    let full = slots(vec![holder("a", "#12 · Work an issue (Implement)")], 0, 1);
    let why = full.full().unwrap();
    assert!(why.contains(&full.said()), "{why}");
    assert!(slots(vec![], 0, 0)
        .full()
        .unwrap()
        .contains("unattended.at_once"));
}

#[test]
fn the_waiting_cap_bites_on_its_own_and_unset_is_none() {
    let mut s = slots(vec![], 0, 2);
    s.waiting = 40;
    assert!(s.full().is_none(), "unset: no cap");
    s.waiting_cap = Some(3);
    s.waiting = 2;
    assert!(s.full().is_none());
    s.waiting = 3;
    let why = s.full().unwrap();
    assert!(why.contains("unattended.waiting"), "{why}");
    // Answering one makes room at once.
    s.waiting = 2;
    assert!(s.full().is_none());
    // The working cap still bites with waiting under its cap.
    s.holders = vec![holder("a", "#1"), holder("b", "#2")];
    assert!(s.full().unwrap().contains("Slots: 2 of 2"));
}

#[test]
fn a_pull_request_closes_the_issue_only_where_the_forge_knows_it() {
    let source = |tracker: TrackerRef, forge_ref: Option<&str>| IssueSource {
        tracker,
        number: 3,
        forge_ref: forge_ref.map(str::to_string),
        forge: Some("Forge".into()),
        base: "origin/main".into(),
        picked: false,
        unsent: Vec::new(),
        notes: Vec::new(),
    };
    let brief = brief_for(&forge(), &issue(3, "Fix it"), "");
    let body = |s: &IssueSource| pull_request_text(&brief, Some(s)).1;
    let on_forge = TrackerRef::Forge {
        connector: "Forge".into(),
    };
    assert!(body(&source(on_forge, None)).starts_with("Closes #3."));
    let synced = TrackerRef::Synced {
        file: "/i.json".into(),
        connector: "Forge".into(),
    };
    assert!(body(&source(synced.clone(), Some("#57"))).starts_with("Closes #57."));
    // Kept here alone, its number is onehand's, not the forge's.
    assert!(!body(&source(synced, None)).contains('#'));
    let kept = TrackerRef::Local {
        file: "/i.json".into(),
    };
    assert!(!body(&source(kept, None)).contains('#'));
    assert_eq!(pull_request_text(&brief, None).0, "Fix it");
}

#[test]
fn a_branch_taken_on_the_forge_is_passed_over_too() {
    let root = std::env::temp_dir();
    let taken = |name: &str| Ok(name == "onehand/github-1-x");
    assert_eq!(
        free_branch_blocking(&root, "onehand/github-1-x", taken),
        Ok("onehand/github-1-x-2".to_string())
    );
    assert_eq!(
        free_branch_blocking(&root, "onehand/github-2-x", taken),
        Ok("onehand/github-2-x".to_string())
    );
    // A forge that cannot be asked is not read as one with nothing on it.
    assert_eq!(
        free_branch_blocking(&root, "onehand/github-3-x", |_| Err("offline".into())),
        Err("offline".to_string())
    );
}

#[test]
fn instructions_for_the_run_follow_the_briefs_own_into_the_first_prompt_and_a_retry() {
    let issue = issue(7, "Fix the parser");
    let added = "Keep the old flag working.";
    let brief = brief_for(&forge(), &issue, added);
    let asked = brief.instructions.clone().unwrap();
    let (own, theirs) = (
        asked.find("Forge issue #7").unwrap(),
        asked.find(added).unwrap(),
    );
    assert!(own < theirs, "the run's own instructions lead: {asked}");
    let template = crate::workflow::builtin::all().remove(0);
    let prompt = crate::workflow::first_prompt(&template, &brief).unwrap();
    assert!(prompt.contains(added), "{prompt}");
    let setup = crate::workflow::Setup {
        repo: "/repo".into(),
        dir: "/repo".into(),
        branch: None,
        agent: None,
        check: None,
        mode: None,
        forge: None,
    };
    let first = crate::workflow::Run::new("1".into(), template.clone(), brief, setup);
    let retry = crate::workflow::Run::retry_of(&first, "2".into(), template, None);
    assert!(retry.brief.instructions.unwrap().contains(added));
    // Nothing typed changes nothing.
    assert_eq!(
        brief_for(&forge(), &issue, "  \n"),
        brief_for(&forge(), &issue, "")
    );
    assert!(!brief_for(&forge(), &issue, "")
        .instructions
        .unwrap()
        .ends_with('\n'));
}

#[test]
fn what_the_start_noted_is_said_last() {
    let noted = PendingReport {
        notes: vec!["An earlier task on this issue ended: too many misses at Implement. Its worktree is kept.".into()],
        ..ran(Outcome::Done)
    };
    let said = report(&noted, &Ok(Verdict::Commits(1)), "b");
    assert!(said.ends_with("Its worktree is kept."), "{said}");
}

#[test]
fn what_the_start_noted_is_said_even_when_the_run_could_not_start() {
    let refused = PendingReport {
        started: false,
        notes: vec!["The issue has no acceptance written.".into()],
        ..ran(Outcome::Failed("the agent offers no mode `x`".into()))
    };
    let said = report(&refused, &Ok(Verdict::Commits(0)), "b");
    assert!(
        said.starts_with("onehand could not start the run"),
        "{said}"
    );
    assert!(
        said.ends_with("The issue has no acceptance written."),
        "{said}"
    );
}
