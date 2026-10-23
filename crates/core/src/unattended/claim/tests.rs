use super::*;
use crate::connector::fake::Fake;
use crate::workflow::{Place, StepKind, StepSpec};

/// A scratch git repository on `main` with one commit.
fn repo(name: &str) -> PathBuf {
    let git = |dir: &Path, args: &[&str]| {
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
    };
    let repo = std::env::temp_dir().join(format!("onehand-claim-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&repo);
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "t@example.com"]);
    git(&repo, &["config", "user.name", "T"]);
    std::fs::write(repo.join("a.txt"), "x").unwrap();
    git(&repo, &["add", "a.txt"]);
    git(&repo, &["commit", "-qm", "one"]);
    repo
}

fn earlier(number: u64, branch: &str, working: bool, answers: bool) -> Earlier {
    Earlier {
        task: format!("t{number}"),
        tracker: TrackerRef::Forge {
            connector: "Forge".into(),
        },
        number,
        dir: PathBuf::from("/w"),
        branch: branch.into(),
        working,
        answers_reviews: answers,
    }
}

/// A tracker over a scratch issue file of its own, holding `issues`.
fn local(name: &str, issues: &[(&str, &[&str])]) -> (Tracker, PathBuf) {
    let dir = std::env::temp_dir().join(format!("onehand-claim-{name}-{}", std::process::id()));
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

/// A worktree workflow called `id`: an agent turn that commits, then the
/// project's check command when `needs_check`.
fn template(id: &str, needs_check: bool) -> Template {
    use crate::workflow::GateKind;
    let mut t = Template::blank(id);
    t.id = id.to_string();
    t.place = Place::Worktree;
    t.steps = vec![StepSpec {
        id: "work".into(),
        label: "Work".into(),
        kind: StepKind::Agent {
            prompt: "go".into(),
            gates: vec![GateKind::Committed],
            keep_answer: false,
            mode: None,
            options: std::collections::BTreeMap::new(),
        },
    }];
    if needs_check {
        t.steps.push(StepSpec {
            id: "check".into(),
            label: "Check".into(),
            kind: StepKind::Command {
                command: None,
                on_fail: "work".into(),
            },
        });
    }
    assert!(crate::workflow::validate(&t).is_empty());
    t
}

#[test]
fn an_issue_nobody_worked_is_fresh() {
    let t = Tracker::Forge(&Fake::SERVING);
    assert!(matches!(
        taking_blocking(&[], &t, 4, Path::new("/"), Some(&Fake::SERVING)),
        Ok(Taking::Fresh)
    ));
}

#[test]
fn a_busy_issue_is_skipped() {
    let t = Tracker::Forge(&Fake::SERVING);
    assert!(matches!(
        taking_blocking(
            &[earlier(4, "b", true, true)],
            &t,
            4,
            Path::new("/"),
            Some(&Fake::SERVING)
        ),
        Err(Skip::Busy)
    ));
}

#[test]
fn the_pull_request_state_decides_the_taking() {
    static OPEN: Fake = Fake {
        pulls: &[("b", PrState::Open, "u/1")],
        ..Fake::SERVING
    };
    static CLOSED: Fake = Fake {
        pulls: &[("b", PrState::Closed, "u/1")],
        ..Fake::SERVING
    };
    static MERGED: Fake = Fake {
        pulls: &[("b", PrState::Merged, "u/1")],
        ..Fake::SERVING
    };
    let t = Tracker::Forge(&OPEN);
    let root = Path::new("/");
    let last = [earlier(4, "b", false, true)];
    assert!(matches!(
        taking_blocking(&last, &t, 4, root, Some(&OPEN)),
        Ok(Taking::Review { pr, .. }) if pr == "u/1"
    ));
    assert!(matches!(
        taking_blocking(&[earlier(4, "b", false, false)], &t, 4, root, Some(&OPEN)),
        Ok(Taking::Refused(_))
    ));
    assert!(matches!(
        taking_blocking(&last, &t, 4, root, Some(&CLOSED)),
        Ok(Taking::Refused(_))
    ));
    assert!(matches!(
        taking_blocking(&last, &t, 4, root, Some(&MERGED)),
        Ok(Taking::Fresh)
    ));
    // Without a forge there is no pull request to ask about.
    assert!(matches!(
        taking_blocking(&last, &t, 4, root, None),
        Ok(Taking::Fresh)
    ));
}

#[test]
fn a_synced_project_is_searched_in_its_file_only() {
    use crate::connector::memory::Forge;
    static FORGE: std::sync::OnceLock<Forge> = std::sync::OnceLock::new();
    let forge = FORGE.get_or_init(|| Forge::with(Vec::new()));
    let (_, dir) = local("synced", &[("x", &[])]);
    let file = dir.join("issues.json");
    crate::issues::update_blocking(&file, |kept| {
        kept.sync_with(Some(forge.name().to_string()));
        Ok(())
    })
    .unwrap();
    let found = trackers_blocking(Some(file.clone()), Some(forge));
    assert!(matches!(found.as_slice(), [Tracker::Synced { .. }]));

    let (_, other) = local("unsynced", &[("x", &[])]);
    let found = trackers_blocking(Some(other.join("issues.json")), Some(forge));
    assert!(matches!(
        found.as_slice(),
        [Tracker::Local(_), Tracker::Forge(_)]
    ));
    assert!(trackers_blocking(None, None).is_empty());
    let _ = std::fs::remove_dir_all(dir);
    let _ = std::fs::remove_dir_all(other);
}

#[test]
fn choosing_knows_which_workflows_need_a_check() {
    let validated = vec![
        ("a".to_string(), Ok(template("a", true))),
        ("b".to_string(), Ok(template("b", false))),
        ("c".to_string(), Err("broken".to_string())),
    ];
    let by_label = BTreeMap::from([
        ("fast".to_string(), "b".to_string()),
        ("bad".to_string(), "c".to_string()),
    ]);
    let choosing = Choosing::new("a".into(), by_label, &validated);
    assert!(choosing.needs_check(&[], "auto"));
    assert!(!choosing.needs_check(&["fast".into()], "auto"));
    // A workflow that cannot run is not one that needs a check.
    assert!(!choosing.needs_check(&["bad".into()], "auto"));
    assert_eq!(choosing.workflow_for(&["auto".into()], "auto"), "a");
}

#[test]
fn the_look_refuses_what_cannot_be_worked() {
    static NO_ACCOUNT: Fake = Fake {
        account: Err("sign in first"),
        ..Fake::SERVING
    };
    static ELSEWHERE: Fake = Fake {
        name: "Elsewhere",
        serves: Err("not here"),
        ..Fake::SERVING
    };
    let all: &[&'static dyn Connector] = &[&NO_ACCOUNT, &ELSEWHERE];
    let git = repo("look");
    let (accounts, checked) = look_blocking(
        all,
        vec![
            Project {
                root: PathBuf::from("/nowhere-not-git"),
                issues: None,
            },
            Project {
                root: git.clone(),
                issues: None,
            },
            Project {
                root: git.clone(),
                issues: Some(git.join("issues.json")),
            },
        ],
    );
    assert_eq!(accounts[0].1, Err("sign in first".to_string()));
    let why = |at: usize| checked[at].1.as_ref().err().cloned();
    assert_eq!(why(0), Some("it is not a git repository".to_string()));
    assert_eq!(why(1), Some("sign in first".to_string()));
    // Its own issues do not rescue a failed account.
    assert_eq!(why(2), Some("sign in first".to_string()));
    let _ = std::fs::remove_dir_all(git);
}

#[test]
fn a_project_no_connector_serves_works_its_own_issues() {
    static ELSEWHERE: Fake = Fake {
        name: "Elsewhere",
        serves: Err("not here"),
        ..Fake::SERVING
    };
    let git = repo("own");
    let (_, checked) = look_blocking(
        &[&ELSEWHERE],
        vec![
            Project {
                root: git.clone(),
                issues: Some(git.join("i.json")),
            },
            Project {
                root: git.clone(),
                issues: None,
            },
        ],
    );
    assert!(matches!(checked[0].1, Ok(None)));
    assert_eq!(
        checked[1].1.as_ref().err().cloned(),
        Some("not here; and its workspace keeps no issues of its own to work instead".to_string())
    );
    let _ = std::fs::remove_dir_all(git);
}

#[test]
fn a_fresh_claim_without_a_forge_cuts_a_worktree_off_the_checked_out_branch() {
    let git = repo("cut");
    let tracker = Tracker::Local(git.join("issues.json"));
    let claimed = prepare_blocking(
        git.clone(),
        tracker,
        None,
        Issue::new(1, "Fix it".into(), String::new()),
        "w".into(),
        Taking::Fresh,
    )
    .ok()
    .unwrap();
    let Work::Cut { branch, base, dir } = claimed.work else {
        panic!("a fresh claim cuts a worktree")
    };
    assert_eq!(base, "main");
    assert_eq!(branch, "onehand/local-1-fix-it");
    assert!(dir.exists());
    let _ = std::fs::remove_dir_all(dir);
    let _ = std::fs::remove_dir_all(git);
}

#[test]
fn a_refused_taking_is_unstarted_with_its_reason() {
    let r = prepare_blocking(
        PathBuf::from("/"),
        Tracker::Local(PathBuf::from("/none.json")),
        None,
        Issue::new(1, "x".into(), String::new()),
        "w".into(),
        Taking::Refused("no".into()),
    );
    assert_eq!(
        r.err().map(|u| (u.number, u.why)),
        Some((1, "no".to_string()))
    );
}

#[test]
fn a_panic_while_preparing_becomes_unstarted() {
    static PANICS: Fake = Fake {
        default_branch: None,
        ..Fake::SERVING
    };
    let r = prepare_blocking(
        PathBuf::from("/"),
        Tracker::Forge(&PANICS),
        Some(&PANICS),
        Issue::new(1, "x".into(), String::new()),
        "w".into(),
        Taking::Fresh,
    );
    assert_eq!(
        r.err().map(|u| u.why),
        Some("onehand panicked while preparing the worktree".to_string())
    );
}

#[test]
fn the_search_passes_over_what_needs_a_check_and_takes_the_next() {
    let git = repo("search");
    let file = git.join("issues.json");
    for (title, labels) in [
        ("Needs it", &["auto", "needs-check"][..]),
        ("Plain", &["auto"][..]),
    ] {
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
    let validated = vec![
        ("plain".to_string(), Ok(template("plain", false))),
        ("cmd".to_string(), Ok(template("cmd", true))),
    ];
    let choosing = Choosing::new(
        "plain".into(),
        BTreeMap::from([("needs-check".to_string(), "cmd".to_string())]),
        &validated,
    );
    let judging = Judging {
        common: Facts {
            workflow: Err("none yet".into()),
            agent: Some("a".into()),
            agent_configured: true,
            mode: None,
            offered: None,
            has_check: false,
            in_git: false,
            checked_out: None,
            forge: None,
            forge_read: true,
            issue: None,
            slots: None,
            queued_behind: None,
            review: None,
            shared_checkout: None,
        },
        templates: validated,
        tasks: Vec::new(),
    };
    let workable = Workable {
        project: Project {
            root: git.clone(),
            issues: Some(file.clone()),
        },
        forge: None,
        has_check: false,
    };
    let mut checked = Vec::new();
    let found = find_blocking(&[workable], "auto", &choosing, &[], &judging, &mut checked);
    let claimed = claim_found_blocking(found.unwrap(), "auto")
        .unwrap()
        .ok()
        .unwrap();
    assert_eq!(claimed.issue.number, 2);
    assert_eq!(claimed.workflow, "plain");
    assert!(checked.is_empty());
    // The one taken lost its label; the one passed over keeps it.
    let kept = crate::issues::load_blocking(&file).unwrap();
    let labels = |n: u64| kept.get(n).unwrap().labels.clone();
    assert!(!labels(2).contains(&"auto".to_string()));
    assert!(labels(1).contains(&"auto".to_string()));
    if let Work::Cut { dir, .. } = claimed.work {
        let _ = std::fs::remove_dir_all(dir);
    }
    let _ = std::fs::remove_dir_all(git);
}

#[test]
fn a_project_with_nothing_to_list_says_why() {
    static ELSEWHERE: Fake = Fake {
        name: "Elsewhere",
        serves: Err("not here"),
        ..Fake::SERVING
    };
    assert_eq!(
        pickable_blocking(&[&ELSEWHERE], Path::new("/"), None).err(),
        Some("not here".to_string())
    );
    assert_eq!(
        pickable_blocking(&[], Path::new("/"), None).err(),
        Some("no connector is built into this onehand".to_string())
    );
}

#[test]
fn a_forge_that_cannot_be_read_is_said_beside_the_projects_own_issues() {
    static DOWN: Fake = Fake {
        lists: Err("the forge is down"),
        ..Fake::SERVING
    };
    let (tracker, dir) = local("beside", &[("Mine", &[])]);
    let Tracker::Local(file) = &tracker else {
        panic!("a local tracker")
    };
    let (rows, cut, unread) = pickable_blocking(&[&DOWN], &dir, Some(file.clone())).unwrap();
    assert_eq!(rows.len(), 1);
    assert!(!cut);
    assert_eq!(unread, Some("the forge is down".to_string()));
    // With nothing of its own, the forge being down is the whole answer.
    assert_eq!(
        pickable_blocking(&[&DOWN], &dir, None).err(),
        Some("the forge is down".to_string())
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn one_issue_is_picked_from_the_projects_own_only() {
    let (tracker, dir) = local("one", &[("A", &[]), ("B", &[])]);
    let Tracker::Local(file) = &tracker else {
        panic!("a local tracker")
    };
    let (rows, _, _) = pickable_one_blocking(&[], &dir, Some(file.clone()), 2).unwrap();
    assert_eq!(rows[0].1.issue.number, 2);
    assert!(pickable_one_blocking(&[], &dir, Some(file.clone()), 9)
        .err()
        .is_some_and(|why| why.contains("cannot be worked by a run")));
    assert_eq!(
        pickable_one_blocking(&[], &dir, None, 1).err(),
        Some("this workspace keeps no issues of its own".to_string())
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_checkout_workflow_is_unfit_for_an_issue() {
    let mut checkout = template("c", false);
    checkout.place = Place::Checkout;
    let why = crate::preflight::unfit_for_issue(&checkout);
    assert!(why.is_some());
    assert_eq!(fit_for_issue(checkout).err(), why);
    // One that validates and works on a worktree comes back as it was.
    assert_eq!(
        fit_for_issue(template("w", false)),
        Ok(template("w", false))
    );
    // One that does not validate says every problem.
    let mut broken = template("b", false);
    broken.steps.clear();
    assert_eq!(
        fit_for_issue(broken).err(),
        Some("the workflow `b` cannot run: it has no steps".to_string())
    );
}

#[test]
fn cannot_start_says_the_first_thing_that_stops_every_run() {
    let ok = Ok(template("w", false));
    assert_eq!(
        cannot_start(Some("mode"), &ok, &[], true),
        Some("mode".into())
    );
    assert_eq!(
        cannot_start(None, &Err("gone".into()), &[], true),
        Some("gone (unattended.workflow)".into())
    );
    assert_eq!(
        cannot_start(None, &ok, &[("l".into(), Err("bad".into()))], true),
        Some("bad (unattended.workflows, label `l`)".into())
    );
    assert_eq!(
        cannot_start(None, &ok, &[], false),
        Some("no agent is configured to run it".into())
    );
    assert_eq!(
        cannot_start(None, &ok, &[("l".into(), ok.clone())], true),
        None
    );
}

#[test]
fn now_for_takes_the_runs_settings_for_an_issue_only() {
    let runs = RunSettings {
        agent: None,
        mode: "  ".into(),
        timeout: "1h".into(),
    };
    let n = now_for(
        true,
        Some(&runs),
        Some("first".into()),
        Some("make test".into()),
    );
    assert_eq!(
        (
            n.agent.as_deref(),
            n.mode,
            n.timeout.as_deref(),
            n.check.as_deref()
        ),
        (Some("first"), None, Some("1h"), Some("make test"))
    );
    let n = now_for(false, Some(&runs), Some("first".into()), None);
    assert_eq!((n.agent.as_deref(), n.timeout), (Some("first"), None));
    // An issue's task with runs not set up is given what a launcher's is.
    let n = now_for(true, None, Some("first".into()), None);
    assert_eq!(
        (n.agent.as_deref(), n.mode, n.timeout),
        (Some("first"), None, None)
    );
    let runs = RunSettings {
        agent: Some("own".into()),
        mode: "plan".into(),
        timeout: "1h".into(),
    };
    let n = now_for(true, Some(&runs), Some("first".into()), None);
    assert_eq!(
        (n.agent.as_deref(), n.mode.as_deref()),
        (Some("own"), Some("plan"))
    );
}

#[test]
fn check_missing_says_where_to_set_one() {
    let t = template("cmd", true);
    assert!(check_missing(&t, false)
        .unwrap()
        .ends_with("; set one on the project's page"));
    assert_eq!(check_missing(&t, true), None);
    assert_eq!(check_missing(&template("plain", false), false), None);
}

/// A task taken up on the issue between the search and the claim (a Retry
/// pressed meanwhile) leaves it unclaimed; one that ended does not.
#[test]
fn a_task_taken_up_after_the_search_keeps_the_issue_unclaimed() {
    static FORGE: Fake = Fake {
        name: "Forge",
        ..Fake::SERVING
    };
    let tracker = Tracker::Forge(&FORGE);
    assert!(still_free(&[], &tracker, 7));
    assert!(still_free(&[earlier(7, "b", false, true)], &tracker, 7));
    assert!(!still_free(&[earlier(7, "b", true, true)], &tracker, 7));
    // Another issue's task is no matter, and the newest task on this one is
    // what counts.
    assert!(still_free(&[earlier(8, "b", true, true)], &tracker, 7));
    assert!(still_free(
        &[earlier(7, "a", true, true), earlier(7, "b", false, true)],
        &tracker,
        7
    ));
}
