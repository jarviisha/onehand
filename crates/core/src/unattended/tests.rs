use super::*;
use crate::connector::fake::Fake;

/// The test forge, as the tracker an issue on it lives in.
fn forge() -> Tracker {
    Tracker::Forge(&Fake::SERVING)
}

/// A fresh attempt's progress.
fn fresh() -> Progress {
    Progress::new(Start::Fresh, None)
}

/// [`step_prompt`] on branch `b` with no check output.
fn prompt(
    step: Step,
    issue: &Issue,
    tracker: &Tracker,
    forge: Option<&dyn Connector>,
    progress: &Progress,
) -> String {
    let branch = if issue.number == 42 {
        "onehand/issue-42"
    } else {
        "b"
    };
    step_prompt(step, issue, branch, tracker, forge, progress, None)
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
fn a_duration_is_said_the_way_it_is_written() {
    for text in ["30m", "2h", "90s"] {
        assert_eq!(spoken(parse_every(text).unwrap()), text);
    }
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
        let branch = branch_for(&issue(7, title));
        assert!(
            crate::worktree::validate_branch(&branch).is_ok(),
            "{title:?} gave {branch:?}"
        );
        assert!(branch.starts_with("onehand/issue-7"));
        assert!(branch.len() <= "onehand/issue-7-".len() + 40);
    }
    assert_eq!(
        branch_for(&issue(3, "Fix the rail!")),
        "onehand/issue-3-fix-the-rail"
    );
    assert_eq!(branch_for(&issue(3, "!!!")), "onehand/issue-3");
}

#[test]
fn the_prompt_names_the_issue_and_the_branch_and_keeps_the_body_whole() {
    let body = "Steps:\n\n```rust\nfn main() {}\n```\n\nThat is all.";
    let issue = Issue {
        body: body.to_string(),
        ..issue(42, "Crash on open")
    };
    let prompt = prompt(
        Step::OpenPr,
        &issue,
        &forge(),
        Some(&Fake::SERVING),
        &fresh(),
    );
    assert!(prompt.contains("#42"));
    assert!(prompt.contains("`onehand/issue-42`"));
    assert!(prompt.contains(body));
    assert!(prompt.contains("Work Forge issue") && prompt.contains("`forge pr`"));
}

#[test]
fn the_claim_stays_true_if_nothing_follows_it() {
    let said = claim_comment("auto");
    assert!(said.contains("started"));
    assert!(said.contains("re-add `auto`"));
    assert!(!said.contains("is working"));
}

#[test]
fn every_ending_has_a_sentence_with_and_without_a_pr() {
    let endings = [
        Ending::TurnEnded {
            tail: Some("Should I use A or B?".into()),
        },
        Ending::TurnEnded { tail: None },
        Ending::Asked("Run rm -rf target?".into()),
        Ending::LinkLost,
        Ending::Closed,
        Ending::TimedOut(Duration::from_secs(2700)),
        Ending::TakenOver,
        Ending::Failed("git refused".into()),
        Ending::Ready { checks_ran: true },
        Ending::Ready { checks_ran: false },
        Ending::Exhausted(Spent::Turns(3, Step::Plan)),
        Ending::Exhausted(Spent::Unapproved(Duration::from_secs(3600))),
        Ending::Exhausted(Spent::FailedAgain(vec!["Clippy".into()])),
        Ending::PullRequestGone,
    ];
    for ending in &endings {
        // Exhaustive on purpose: a new ending cannot be added without being
        // listed above, and so without a sentence being checked for it.
        match ending {
            Ending::TurnEnded { .. }
            | Ending::Asked(_)
            | Ending::LinkLost
            | Ending::Closed
            | Ending::TimedOut(_)
            | Ending::TakenOver
            | Ending::Failed(_)
            | Ending::Ready { .. }
            | Ending::Exhausted(_)
            | Ending::PullRequestGone => {}
        }
        let without = report(ending, &Ok(Verdict::NoPullRequest), "onehand/issue-1");
        assert!(!without.is_empty());
        assert!(!without.contains("opened"), "{without}");
        let with = report(
            ending,
            &Ok(Verdict::PullRequest("https://x/pull/2".into())),
            "onehand/issue-1",
        );
        assert!(
            with.starts_with("onehand opened https://x/pull/2."),
            "{with}"
        );
        let unknown = report(ending, &Err("gh: offline".into()), "onehand/issue-1");
        assert!(unknown.contains("gh: offline"), "{unknown}");
    }
}

#[test]
fn a_pr_found_after_a_timeout_is_still_the_verdict() {
    let said = report(
        &Ending::TimedOut(Duration::from_secs(2700)),
        &Ok(Verdict::PullRequest("https://x/pull/2".into())),
        "b",
    );
    assert!(said.starts_with("onehand opened https://x/pull/2."));
    assert!(said.contains("45m timeout"));
}

#[test]
fn a_pr_lookup_that_failed_never_reads_as_no_pr() {
    for ending in [
        Ending::TurnEnded { tail: None },
        Ending::LinkLost,
        Ending::TimedOut(Duration::from_secs(60)),
        Ending::TakenOver,
    ] {
        let said = report(&ending, &Err("rate limited".into()), "b");
        assert!(!said.contains("no pull request"), "{said}");
        assert!(said.contains("could not tell"), "{said}");
    }
}

#[test]
fn a_picked_claim_says_how_to_retry_without_a_label() {
    let said = picked_claim_comment();
    assert!(said.contains("started") && said.contains("picked by hand"));
    assert!(
        !said.contains("re-add"),
        "a picked issue may never have had the label"
    );
}

#[test]
fn the_outcome_fits_on_one_line_and_leads_with_the_pr() {
    let pr = Ok(Verdict::PullRequest("https://x/pull/2".to_string()));
    let line = outcome_line(&Ending::TimedOut(Duration::from_secs(60)), &pr);
    assert!(line.starts_with("Opened https://x/pull/2"), "{line}");
    let none = outcome_line(
        &Ending::TurnEnded {
            tail: Some("long\nanswer".into()),
        },
        &Ok(Verdict::NoPullRequest),
    );
    assert!(
        !none.contains('\n') && none.contains("no pull request"),
        "{none}"
    );
    let unknown = outcome_line(&Ending::LinkLost, &Err("offline".into()));
    assert!(unknown.contains("could not tell"), "{unknown}");
    for line in [line, none, unknown] {
        assert!(line.chars().count() <= 80, "{line}");
    }
}

#[test]
fn a_question_in_prose_reaches_the_issue() {
    let said = report(
        &Ending::TurnEnded {
            tail: Some("Should I use A\nor B?".into()),
        },
        &Ok(Verdict::NoPullRequest),
        "b",
    );
    assert!(said.contains("> Should I use A\n> or B?"));
}

#[test]
fn an_empty_label_picks_nothing_without_asking() {
    let nowhere = std::env::temp_dir();
    assert_eq!(candidate_blocking(&forge(), &nowhere, "", &[]), Ok(None));
    assert_eq!(candidate_blocking(&forge(), &nowhere, "   ", &[]), Ok(None));
}

#[test]
fn the_oldest_issue_is_taken_first() {
    let found = candidate_blocking(&forge(), &std::env::temp_dir(), "auto", &[]).unwrap();
    assert_eq!(found.map(|i| i.number), Some(4));
    static NONE_LABELLED: Fake = Fake {
        labelled: &[],
        ..Fake::SERVING
    };
    assert_eq!(
        candidate_blocking(
            &Tracker::Forge(&NONE_LABELLED),
            &std::env::temp_dir(),
            "auto",
            &[]
        ),
        Ok(None)
    );
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
    let found = candidate_blocking(&tracker, &root, "auto", &[])
        .unwrap()
        .unwrap();
    assert_eq!(found.number, 1, "the oldest labelled issue goes first");
    claim_blocking(&tracker, &root, 1, "auto").unwrap();
    let Tracker::Local(file) = &tracker else {
        unreachable!()
    };
    let kept = crate::issues::load_blocking(file).unwrap();
    let claimed = kept.get(1).unwrap();
    assert!(claimed.labels.is_empty(), "the label is the claim");
    assert!(claimed.notes[0].text.contains("started"));
    assert_eq!(
        candidate_blocking(&tracker, &root, "auto", &[])
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
fn a_local_issue_is_never_referenced_from_a_pull_request() {
    let (tracker, dir) = local("prompt", &[]);
    let issue = issue(3, "Fix it");
    let on_forge = prompt(
        Step::OpenPr,
        &issue,
        &tracker,
        Some(&Fake::SERVING),
        &fresh(),
    );
    assert!(on_forge.contains("`forge pr`"), "{on_forge}");
    assert!(on_forge.contains("Do not reference #3"), "{on_forge}");
    assert!(!on_forge.contains("referencing #3"), "{on_forge}");
    let no_forge = prompt(Step::OpenPr, &issue, &tracker, None, &fresh());
    assert!(no_forge.contains("Do not push"), "{no_forge}");
    assert!(!no_forge.contains("pull request"), "{no_forge}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn commits_left_on_a_branch_are_the_verdict_without_a_forge() {
    let said = report(
        &Ending::TurnEnded { tail: None },
        &Ok(Verdict::Commits(2)),
        "b",
    );
    assert_eq!(said, "onehand left 2 commits on `b`.");
    let one = outcome_line(
        &Ending::TimedOut(Duration::from_secs(60)),
        &Ok(Verdict::Commits(1)),
    );
    assert_eq!(one, "Left 1 commit on its branch");
    let none = report(
        &Ending::TimedOut(Duration::from_secs(60)),
        &Ok(Verdict::NoCommits),
        "b",
    );
    assert!(none.starts_with("No commit after 1m"), "{none}");
    assert!(!none.contains("pull request"), "{none}");
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
    let first = candidate_blocking(&tracker, &root, "auto", &[])
        .unwrap()
        .unwrap();
    assert_eq!((first.number, first.forge_ref()), (1, None));
    claim_blocking(&tracker, &root, 1, "auto").unwrap();
    let second = candidate_blocking(&tracker, &root, "auto", &[])
        .unwrap()
        .unwrap();
    assert_eq!(second.forge_ref(), Some("#7"));
    claim_blocking(&tracker, &root, second.number, "auto").unwrap();
    assert_eq!(
        candidate_blocking(&tracker, &root, "auto", &[]).unwrap(),
        None
    );

    // The claim reached the forge: the label came off and it was told.
    assert!(forge.said("7").labels.is_empty());
    let comments = forge.comments.lock().unwrap().clone();
    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0].0, "7");
    assert!(comments[0].1.contains("started"));

    // And its pull request names the forge's number, not onehand's.
    let prompt = prompt(Step::OpenPr, &second, &tracker, Some(forge), &fresh());
    assert!(prompt.contains("referencing #7"), "{prompt}");
    assert!(prompt.contains("Forge issue #7"), "{prompt}");
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
    assert_eq!(candidate_blocking(&synced, &root, "auto", &[]), Ok(None));
    assert_eq!(candidate_blocking(&local, &root, "auto", &[]), Ok(None));
    // Nor once the forge has lost it and it is kept here unlinked.
    forge.issues.lock().unwrap().clear();
    crate::issues::sync::sync_blocking(&file, &root, forge, 2).unwrap();
    assert_eq!(candidate_blocking(&synced, &root, "auto", &[]), Ok(None));
    assert_eq!(candidate_blocking(&local, &root, "auto", &[]), Ok(None));
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
fn a_run_that_ended_waiting_says_the_question() {
    let asked = Ending::Asked("Run awk?".into());
    let said = report(&asked, &Ok(Verdict::NoPullRequest), "b");
    assert!(said.contains("> Run awk?"), "{said}");
    let quiet = report(&Ending::TakenOver, &Ok(Verdict::NoPullRequest), "b");
    assert!(!quiet.contains('>'), "{quiet}");
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
fn an_issue_a_run_is_still_on_is_passed_over() {
    let found = candidate_blocking(&forge(), &std::env::temp_dir(), "auto", &[4]).unwrap();
    assert_eq!(found.map(|i| i.number), Some(9));
}

#[test]
fn a_branch_keeps_its_issue_prefix_whatever_the_title() {
    for title in ["Fix the rail", "!!!", ""] {
        assert!(branch_for(&issue(12, title)).starts_with(&issue_prefix(12)));
    }
}

#[test]
fn a_session_on_an_open_pull_request_pushes_to_it_rather_than_opening_another() {
    let start = Start::Review {
        pr: "https://x/pull/2".into(),
        number: 2,
    };
    let progress = Progress::new(start, None);
    let said = prompt(
        Step::OpenPr,
        &issue(4, "t"),
        &forge(),
        Some(&Fake::SERVING),
        &progress,
    );
    assert!(said.contains("`forge review 2`"), "{said}");
    assert!(said.contains("do not open another"), "{said}");
    assert!(!said.contains("`forge pr`"), "{said}");
}

#[test]
fn a_budget_carried_into_a_later_session_keeps_what_was_spent() {
    let now = Instant::now();
    let budget = Budget::resumed(Duration::from_secs(60), Duration::from_secs(50), now);
    assert_eq!(budget.left(now), Duration::from_secs(10));
    assert_eq!(budget.spent(now), Duration::from_secs(50));
}

#[test]
fn a_mode_written_by_hand_is_still_offered_as_the_one_in_force() {
    assert_eq!(mode_choices("auto").len(), MODES.len());
    let choices = mode_choices("dontAsk");
    assert_eq!(
        choices.last(),
        Some(&("dontAsk".to_string(), "dontAsk".to_string()))
    );
}

#[test]
fn each_step_prompt_names_its_own_rules() {
    let issue = issue(5, "t");
    let tracker = forge();
    let forge = Some(&Fake::SERVING as &dyn Connector);
    let mut progress = fresh();
    let plan = prompt(Step::Plan, &issue, &tracker, forge, &progress);
    assert!(plan.contains("Do not edit any file"), "{plan}");
    progress.plan = Some("Change the rail.".into());
    progress.revise = Some("Smaller, please.".into());
    let revised = prompt(Step::Plan, &issue, &tracker, forge, &progress);
    assert!(revised.contains("> Smaller, please."), "{revised}");
    assert!(revised.contains("> Change the rail."), "{revised}");
    let implement = prompt(Step::Implement, &issue, &tracker, forge, &progress);
    assert!(implement.contains("> Change the rail."), "{implement}");
    assert!(implement.contains("Do not push"), "{implement}");
    let verify = step_prompt(
        Step::Verify,
        &issue,
        "b",
        &tracker,
        forge,
        &progress,
        Some("error: boom"),
    );
    assert!(verify.contains("```\nerror: boom\n```"), "{verify}");
    let open = prompt(Step::OpenPr, &issue, &tracker, forge, &progress);
    assert!(open.contains("`forge pr`") && open.contains("#5"), "{open}");
}

#[test]
fn a_check_passes_fails_and_hands_back_only_its_last_lines() {
    let dir = std::env::temp_dir();
    assert_eq!(verify_blocking(&dir, "true"), Ok(()));
    let failed = verify_blocking(&dir, "echo out; echo err >&2; false").unwrap_err();
    assert_eq!(failed, "out\nerr");
    let long = verify_blocking(&dir, "seq 1 500; exit 3").unwrap_err();
    assert_eq!(long.lines().count(), VERIFY_LINES);
    assert!(long.starts_with("301\n") && long.ends_with("500"), "{long}");
    let silent = verify_blocking(&dir, "false").unwrap_err();
    assert!(silent.contains("printed nothing"), "{silent}");
}

#[test]
fn a_step_note_stays_in_onehand() {
    let (tracker, dir) = local("note", &[("Fix it", &[])]);
    tracker.note_blocking(1, "Step: Implement").unwrap();
    let Tracker::Local(file) = &tracker else {
        unreachable!()
    };
    let kept = crate::issues::load_blocking(file).unwrap();
    assert_eq!(kept.get(1).unwrap().notes[0].text, "Step: Implement");
    assert_eq!(forge().note_blocking(1, "Step: Plan"), Ok(()));
    let _ = std::fs::remove_dir_all(dir);
}
