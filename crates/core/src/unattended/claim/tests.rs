use super::*;
use crate::connector::fake::Fake;
use crate::workflow::{Place, StepKind, StepSpec};

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

/// A worktree workflow called `id` with one step: the project's check command
/// when `needs_check`, an agent turn otherwise.
fn template(id: &str, needs_check: bool) -> Template {
    let mut t = Template::blank(id);
    t.id = id.to_string();
    t.place = Place::Worktree;
    t.steps = vec![StepSpec {
        id: "s".into(),
        label: "s".into(),
        kind: match needs_check {
            true => StepKind::Command {
                command: None,
                on_fail: "s".into(),
            },
            false => StepKind::Agent {
                prompt: "go".into(),
                gates: Vec::new(),
                keep_answer: false,
            },
        },
    }];
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
