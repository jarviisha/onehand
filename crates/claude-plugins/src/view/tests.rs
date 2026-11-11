use super::details::compact;

#[test]
fn a_count_reads_at_a_glance() {
    assert_eq!(compact(7), "7");
    assert_eq!(compact(3327), "3.3k");
    assert_eq!(compact(12_000), "12k");
    assert_eq!(compact(2_450_000), "2.5M");
}

mod carried {
    //! The mode against a `claude` of its own, put first on `PATH`: one change
    //! at a time, what went through counted for the restart offer, a refusal
    //! said only in the project it was made in, and the list read again after.
    //!
    //! Only these tests in this binary start `claude`, so the `PATH` they set
    //! reaches nothing else.

    use super::super::PluginsView;
    use gpui::{Entity, TestAppContext};
    use onehand_core::claude_plugins::cli::{Change, Scope, Verb};
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};

    /// A `claude` that lists nothing, logs every call to `calls.log` beside
    /// it, and refuses any change naming `refused@market`.
    fn fake_claude() -> PathBuf {
        static ONCE: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
        ONCE.get_or_init(|| {
            let dir =
                std::env::temp_dir().join(format!("onehand-plugins-claude-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let script = dir.join("claude");
            std::fs::write(
                &script,
                format!(
                    "#!/bin/sh\necho \"$*\" >> '{log}'\n\
                     case \"$*\" in\n\
                     *refused@market*) echo 'not allowed' >&2; exit 1;;\n\
                     'plugin list'*) echo '{{\"installed\": [], \"available\": []}}';;\n\
                     'plugin marketplace list'*) echo '[]';;\n\
                     esac\n",
                    log = dir.join("calls.log").display()
                ),
            )
            .unwrap();
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
            let path = std::env::var_os("PATH").unwrap_or_default();
            let mut dirs = vec![dir.clone()];
            dirs.extend(std::env::split_paths(&path));
            // SAFETY: set once, before any thread of this binary starts
            // `claude` or reads `PATH`.
            unsafe { std::env::set_var("PATH", std::env::join_paths(dirs).unwrap()) };
            dir
        })
        .clone()
    }

    /// The changes this test's project was asked for, by the fake's log.
    fn changes_in(log: &Path, id: &str) -> usize {
        std::fs::read_to_string(log)
            .unwrap_or_default()
            .lines()
            .filter(|line| line.contains(id) && !line.starts_with("plugin list"))
            .count()
    }

    fn settle(cx: &mut TestAppContext) {
        for _ in 0..30 {
            cx.run_until_parked();
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn view(cx: &mut TestAppContext, root: &Path) -> Entity<PluginsView> {
        cx.executor().allow_parking();
        let view = cx.update(|cx| PluginsView::new(std::rc::Rc::new(|_, _, _| {}), cx));
        view.update(cx, |view, cx| view.set_root(root, cx));
        view
    }

    fn enable(id: &str) -> Change {
        Change {
            id: id.into(),
            scope: Scope::Local,
            verb: Verb::Enable,
        }
    }

    #[gpui::test]
    fn a_change_pressed_while_one_runs_is_not_made(cx: &mut TestAppContext) {
        let log = fake_claude().join("calls.log");
        let view = view(cx, Path::new("/tmp"));
        view.update(cx, |view, cx| {
            view.agent_started(Some(Instant::now() - Duration::from_secs(1)), cx);
            view.change(enable("one@market"), cx);
            view.change(enable("two@market"), cx);
        });
        settle(cx);
        assert_eq!(changes_in(&log, "one@market"), 1);
        assert_eq!(changes_in(&log, "two@market"), 0);
        view.read_with(cx, |view, _| {
            assert!(view.drive.busy().is_none());
            // What went through is one the running agent has not loaded,
            // and the list is read again before it is next drawn.
            assert_eq!(view.drive.pending(), 1);

            assert_eq!(view.drive.refused(), None);
        });
    }

    #[gpui::test]
    fn a_refusal_is_said_in_its_project_and_counts_for_nothing(cx: &mut TestAppContext) {
        fake_claude();
        let view = view(cx, Path::new("/tmp"));
        view.update(cx, |view, cx| {
            view.agent_started(Some(Instant::now() - Duration::from_secs(1)), cx);
            view.change(enable("refused@market"), cx);
        });
        settle(cx);
        view.read_with(cx, |view, _| {
            assert_eq!(view.drive.refused(), Some("not allowed"));
            assert_eq!(view.drive.pending(), 0);
        });

        // Pressed here, refused after the person moved to another project:
        // nothing is said there.
        view.update(cx, |view, cx| {
            view.change(enable("refused@market"), cx);
            view.set_root(Path::new("/"), cx);
        });
        settle(cx);
        view.read_with(cx, |view, _| assert_eq!(view.drive.refused(), None));
    }

    #[gpui::test]
    fn a_global_change_waits_for_the_agent_in_every_project(cx: &mut TestAppContext) {
        fake_claude();
        let view = view(cx, Path::new("/tmp"));
        view.update(cx, |view, cx| {
            view.agent_started(Some(Instant::now() - Duration::from_secs(1)), cx);
            view.change(
                Change {
                    id: "all@market".into(),
                    scope: Scope::User,
                    verb: Verb::Enable,
                },
                cx,
            );
        });
        settle(cx);
        view.update(cx, |view, cx| {
            view.set_root(Path::new("/"), cx);
            assert_eq!(view.drive.pending(), 1);
            // An agent started since has loaded it.
            view.agent_started(Some(Instant::now()), cx);
            assert_eq!(view.drive.pending(), 0);
        });
    }
}
