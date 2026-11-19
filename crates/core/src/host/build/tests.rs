use super::*;
use crate::workspace::{BuildHost, HostName};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// A scratch folder of this test's own, gone when the test ends.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "onehand-build-{}-{}-{name}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).expect("a scratch folder");
        Self(dir.canonicalize().expect("canonical"))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@localhost"])
        .args(args)
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn write(dir: &Path, name: &str, text: &str) {
    let path = dir.join(name);
    std::fs::create_dir_all(path.parent().expect("a parent")).expect("folders");
    std::fs::write(path, text).expect("written");
}

/// A repository at `scratch/<name>` with one commit: `tracked.txt`, and a
/// `.gitignore` leaving `target/` and `secret.env` out.
fn repo(scratch: &Scratch, name: &str) -> PathBuf {
    repo_in(&scratch.0, name)
}

fn repo_in(parent: &Path, name: &str) -> PathBuf {
    let top = parent.join(name);
    std::fs::create_dir_all(&top).expect("the folder");
    git(&top, &["init", "-q", "-b", "main"]);
    write(&top, ".gitignore", "target/\nsecret.env\n");
    write(&top, "tracked.txt", "one\n");
    git(&top, &["add", "-A"]);
    git(&top, &["commit", "-q", "-m", "base"]);
    top
}

fn on_local(artifacts: &[&str], into: Option<&str>) -> BuildHost {
    BuildHost {
        host: HostName("local".into()),
        artifacts: artifacts.iter().map(PathBuf::from).collect(),
        into: into.map(PathBuf::from),
    }
}

/// Build `top` on the host `local`, whose folders are under `home`: the
/// outcome and every line the command printed.
fn build(
    home: &Scratch,
    top: &Path,
    within: &str,
    host: &BuildHost,
    command: &str,
) -> (Result<Built, String>, Vec<String>) {
    let mut lines = Vec::new();
    let job = Job {
        top,
        within: Path::new(within),
        host,
        command,
    };
    let built = build_in(
        &job,
        &home.0,
        "laptop",
        &mut |line| lines.push(line.to_string()),
        &AtomicBool::new(false),
    );
    (built, lines)
}

#[test]
fn quoted_survives_spaces_quotes_and_dollars() {
    for text in [
        "a b",
        "it's",
        "$HOME",
        "a\"b",
        "",
        "-n",
        "semi;colon && more",
    ] {
        let out = Command::new("sh")
            .arg("-c")
            .arg(format!("printf %s {}", quoted(text)))
            .output()
            .expect("sh runs");
        assert_eq!(String::from_utf8_lossy(&out.stdout), text);
    }
}

#[test]
fn the_folder_on_the_host_is_named_safely_and_by_the_laptop() {
    let dir = remote_dir("lap-1", Path::new("/work/my proj's"));
    assert!(dir.starts_with(".onehand-build/lap-1/my_proj_s-"), "{dir}");
    assert_ne!(dir, remote_dir("lap-1", Path::new("/other/my proj's")));
}

#[test]
fn the_command_runs_on_the_snapshot_and_its_lines_come_back() {
    let (home, scratch) = (Scratch::new("home"), Scratch::new("work"));
    let top = repo(&scratch, "proj");
    write(&top, "tracked.txt", "edited\n");
    write(&top, "fresh.txt", "new\n");
    write(&top, "secret.env", "KEY=1\n");
    let before = git(&top, &["status", "--porcelain"]);
    let head = git(&top, &["rev-parse", "HEAD"]);

    let (built, lines) = build(
        &home,
        &top,
        "",
        &on_local(&[], None),
        "cat tracked.txt fresh.txt; test -e secret.env && echo LEAKED; echo to-stderr >&2",
    );
    assert_eq!(
        built,
        Ok(Built {
            exit: Some(0),
            brought: Vec::new(),
            hint: None
        })
    );
    assert_eq!(lines, ["edited", "new", "to-stderr"]);
    // Nothing here moved: not the branch, not the index, not a file.
    assert_eq!(git(&top, &["status", "--porcelain"]), before);
    assert_eq!(git(&top, &["rev-parse", "HEAD"]), head);
}

#[test]
fn a_deleted_file_goes_and_an_ignored_folder_stays() {
    let (home, scratch) = (Scratch::new("home"), Scratch::new("work"));
    let top = repo(&scratch, "proj");
    write(&top, "doomed.txt", "x\n");
    let host = on_local(&[], None);
    let (built, _) = build(
        &home,
        &top,
        "",
        &host,
        "mkdir -p target && echo kept > target/cache",
    );
    assert_eq!(built.map(|b| b.exit), Ok(Some(0)));
    std::fs::remove_file(top.join("doomed.txt")).expect("removed");
    let (_, lines) = build(
        &home,
        &top,
        "",
        &host,
        "cat target/cache; test -e doomed.txt && echo STALE; true",
    );
    assert_eq!(lines, ["kept"]);
}

#[test]
fn artifacts_come_back_where_they_were_and_under_into() {
    let (home, scratch) = (Scratch::new("home"), Scratch::new("work"));
    let top = repo(&scratch, "proj");
    let make = "mkdir -p target/release dist && echo bin > target/release/app && echo page > dist/index.html";
    let (built, _) = build(
        &home,
        &top,
        "",
        &on_local(&["target/release/app", "dist"], None),
        make,
    );
    assert_eq!(
        built.expect("built").brought,
        [PathBuf::from("target/release/app"), PathBuf::from("dist")]
    );
    assert_eq!(
        std::fs::read_to_string(top.join("target/release/app")).unwrap(),
        "bin\n"
    );
    assert_eq!(
        std::fs::read_to_string(top.join("dist/index.html")).unwrap(),
        "page\n"
    );

    let (built, _) = build(
        &home,
        &top,
        "",
        &on_local(&["target/release/app"], Some("out/linux")),
        make,
    );
    assert_eq!(
        built.expect("built").brought,
        [PathBuf::from("out/linux/target/release/app")]
    );
    assert_eq!(
        std::fs::read_to_string(top.join("out/linux/target/release/app")).unwrap(),
        "bin\n"
    );
}

#[test]
fn a_failed_command_brings_nothing_back() {
    let (home, scratch) = (Scratch::new("home"), Scratch::new("work"));
    let top = repo(&scratch, "proj");
    let (built, _) = build(
        &home,
        &top,
        "",
        &on_local(&["made.txt"], None),
        "echo x > made.txt; exit 7",
    );
    assert_eq!(
        built,
        Ok(Built {
            exit: Some(7),
            brought: Vec::new(),
            hint: None
        })
    );
    assert!(!top.join("made.txt").exists());
}

#[test]
fn an_artifact_that_was_not_built_fails_the_build_and_writes_none() {
    let (home, scratch) = (Scratch::new("home"), Scratch::new("work"));
    let top = repo(&scratch, "proj");
    let (built, _) = build(
        &home,
        &top,
        "",
        &on_local(&["made.txt", "never made.txt"], None),
        "echo x > made.txt",
    );
    assert_eq!(
        built,
        Err("never made.txt was not built on local".to_string())
    );
    assert!(!top.join("made.txt").exists());
}

#[test]
fn a_command_not_found_says_whose_path() {
    let (home, scratch) = (Scratch::new("home"), Scratch::new("work"));
    let top = repo(&scratch, "proj");
    let (built, _) = build(
        &home,
        &top,
        "",
        &on_local(&[], None),
        "nosuchtool --version",
    );
    let built = built.expect("ran");
    assert_eq!(built.exit, Some(127));
    assert_eq!(
        built.hint.as_deref(),
        Some("nosuchtool is not on local's PATH for ssh")
    );
}

/// Several words from a command line arrive each quoted; the program is
/// named as the person typed it.
#[test]
fn a_quoted_command_not_found_is_named_without_its_quotes() {
    let (home, scratch) = (Scratch::new("home"), Scratch::new("work"));
    let top = repo(&scratch, "proj");
    let (built, _) = build(
        &home,
        &top,
        "",
        &on_local(&[], None),
        "'nosuchtool' '--version'",
    );
    assert_eq!(
        built.expect("ran").hint.as_deref(),
        Some("nosuchtool is not on local's PATH for ssh")
    );
}

#[test]
fn the_command_runs_within_the_folder_asked() {
    let (home, scratch) = (Scratch::new("home"), Scratch::new("work"));
    let top = repo(&scratch, "proj");
    write(&top, "sub dir/file.txt", "x\n");
    let (built, lines) = build(
        &home,
        &top,
        "sub dir",
        &on_local(&[], None),
        "basename \"$PWD\"; ls",
    );
    assert_eq!(built.map(|b| b.exit), Ok(Some(0)));
    assert_eq!(lines, ["sub dir", "file.txt"]);
}

#[test]
fn a_folder_and_command_with_spaces_and_quotes_build() {
    let (home, scratch) = (Scratch::new("home"), Scratch::new("work"));
    let top = repo(&scratch, "my proj's");
    let (built, lines) = build(
        &home,
        &top,
        "",
        &on_local(&["out dir/a b.txt"], None),
        "mkdir -p 'out dir' && echo \"it's $((1+1))\" > 'out dir/a b.txt' && cat 'out dir/a b.txt'",
    );
    assert_eq!(
        built.expect("built").brought,
        [PathBuf::from("out dir/a b.txt")]
    );
    assert_eq!(lines, ["it's 2"]);
    assert_eq!(
        std::fs::read_to_string(top.join("out dir/a b.txt")).unwrap(),
        "it's 2\n"
    );
}

#[test]
fn a_worktree_builds() {
    let (home, scratch) = (Scratch::new("home"), Scratch::new("work"));
    let top = repo(&scratch, "proj");
    let tree = scratch.0.join("proj-feature");
    git(
        &top,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "feature",
            tree.to_str().unwrap(),
        ],
    );
    write(&tree, "only-here.txt", "w\n");
    let (built, lines) = build(&home, &tree, "", &on_local(&[], None), "cat only-here.txt");
    assert_eq!(built.map(|b| b.exit), Ok(Some(0)));
    assert_eq!(lines, ["w"]);
}

#[test]
fn a_folder_that_is_not_a_working_trees_top_is_refused() {
    let (home, scratch) = (Scratch::new("home"), Scratch::new("work"));
    let top = repo(&scratch, "proj");
    write(&top, "sub/file.txt", "x\n");
    let (built, _) = build(&home, &top.join("sub"), "", &on_local(&[], None), "true");
    assert_eq!(
        built,
        Err(format!(
            "{} is not the top of a git working tree",
            top.join("sub").display()
        ))
    );
}

#[test]
fn a_repository_inside_the_folder_is_refused() {
    let (home, scratch) = (Scratch::new("home"), Scratch::new("work"));
    let top = repo(&scratch, "proj");
    repo_in(&top, "inner");
    let (built, _) = build(&home, &top, "", &on_local(&[], None), "true");
    assert_eq!(
        built,
        Err("inner is a repository inside this one; a project holding one cannot be built on a host yet".to_string())
    );
}

#[test]
fn a_second_build_of_one_folder_is_refused_while_the_first_runs() {
    let (home, scratch) = (Scratch::new("home"), Scratch::new("work"));
    let top = repo(&scratch, "proj");
    let host = on_local(&[], None);
    std::thread::scope(|scope| {
        let first = scope.spawn(|| build(&home, &top, "", &host, "sleep 2").0);
        std::thread::sleep(std::time::Duration::from_millis(700));
        let (second, _) = build(&home, &top, "", &host, "true");
        assert_eq!(
            second,
            Err("A build of proj is already running on local".to_string())
        );
        assert_eq!(first.join().expect("joined").map(|b| b.exit), Ok(Some(0)));
    });
}

#[test]
fn a_stopped_build_ends_and_says_no_exit() {
    let (home, scratch) = (Scratch::new("home"), Scratch::new("work"));
    let top = repo(&scratch, "proj");
    let host = on_local(&["never.txt"], None);
    let cancel = AtomicBool::new(false);
    let started = std::time::Instant::now();
    let built = std::thread::scope(|scope| {
        scope.spawn(|| {
            std::thread::sleep(std::time::Duration::from_millis(600));
            cancel.store(true, Ordering::Relaxed);
        });
        let job = Job {
            top: &top,
            within: Path::new(""),
            host: &host,
            command: "sleep 30",
        };
        build_in(&job, &home.0, "laptop", &mut |_| {}, &cancel)
    });
    assert!(started.elapsed() < std::time::Duration::from_secs(10));
    assert_eq!(
        built,
        Ok(Built {
            exit: None,
            brought: Vec::new(),
            hint: None
        })
    );
}

mod cli {
    use super::super::cli::*;
    use super::*;
    use crate::config::{HostOs, HostSpec};
    use crate::workspace::Workspace;

    /// A config root whose one workspace, `name`, holds `top` as a project
    /// building on `hosts`.
    fn keep(root: &Path, name: &str, top: &Path, hosts: &[&str]) {
        let mut ws = Workspace::seeded(top);
        ws.name = name.to_string();
        ws.roots[0].builds = hosts
            .iter()
            .map(|host| BuildHost {
                host: HostName((*host).into()),
                artifacts: vec!["made.txt".into()],
                into: None,
            })
            .collect();
        let dir = crate::config::workspaces_in(root).join(name);
        std::fs::create_dir_all(&dir).expect("the workspace's folder");
        ws.to_config().save_to(&dir).expect("saved");
    }

    #[test]
    fn a_project_is_found_from_a_folder_inside_it() {
        let (root, scratch) = (Scratch::new("root"), Scratch::new("work"));
        let top = repo(&scratch, "proj");
        write(&top, "crates/app/x.rs", "");
        keep(&root.0, "ws", &top, &["build01"]);
        let asked =
            asked_blocking(&root.0, &[], &top.join("crates/app"), "build01").expect("found");
        assert_eq!(asked.top, top);
        assert_eq!(asked.within, PathBuf::from("crates/app"));
        assert_eq!(asked.project, "proj");
        assert_eq!(asked.entry.host, HostName("build01".into()));
    }

    #[test]
    fn a_worktree_is_found_as_its_project() {
        let (root, scratch) = (Scratch::new("root"), Scratch::new("work"));
        let top = repo(&scratch, "proj");
        let tree = scratch.0.join("proj-feature");
        git(
            &top,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "feature",
                tree.to_str().unwrap(),
            ],
        );
        keep(&root.0, "ws", &top, &["build01"]);
        let asked = asked_blocking(&root.0, &[], &tree, "build01").expect("found");
        assert_eq!(asked.top, tree);
        assert_eq!(asked.within, PathBuf::new());
        assert_eq!(asked.project, "proj");
    }

    #[test]
    fn what_cannot_be_built_is_refused_in_words() {
        let (root, scratch) = (Scratch::new("root"), Scratch::new("work"));
        let refused = |declared: &[HostSpec], cwd: &Path, host: &str| {
            asked_blocking(&root.0, declared, cwd, host).expect_err("refused")
        };
        assert_eq!(
            refused(&[], &scratch.0, "h"),
            format!("{} is not inside a git working tree", scratch.0.display())
        );
        let stray = repo(&scratch, "stray");
        assert_eq!(
            refused(&[], &stray, "h"),
            format!("{} is not a project of any workspace", stray.display())
        );
        let bare = repo(&scratch, "bare");
        keep(&root.0, "a", &bare, &[]);
        assert_eq!(
            refused(&[], &bare, "h"),
            "bare builds on no host; add one on its page"
        );
        let top = repo(&scratch, "proj");
        keep(&root.0, "b", &top, &["build01", "win"]);
        assert_eq!(
            refused(&[], &top, "other"),
            "proj does not build on other; it builds on build01, win"
        );
        let windows = [HostSpec {
            name: HostName("win".into()),
            os: HostOs::Windows,
            arch: "AMD64".into(),
        }];
        assert_eq!(
            refused(&windows, &top, "win"),
            "win runs Windows; nothing builds on a Windows host yet"
        );
    }

    #[test]
    fn the_first_workspace_by_name_that_names_the_host_is_read() {
        let (root, scratch) = (Scratch::new("root"), Scratch::new("work"));
        let top = repo(&scratch, "proj");
        keep(&root.0, "zeta", &top, &["build01"]);
        keep(&root.0, "alpha", &top, &["other"]);
        let asked = asked_blocking(&root.0, &[], &top, "build01").expect("found");
        assert_eq!(asked.entry.host, HostName("build01".into()));
    }

    #[test]
    fn command_line_keeps_one_word_and_quotes_several() {
        let words =
            |words: &[&str]| -> Vec<String> { words.iter().map(|w| w.to_string()).collect() };
        assert_eq!(
            command_line(&words(&["cargo test && ls"])).as_deref(),
            Ok("cargo test && ls")
        );
        assert_eq!(
            command_line(&words(&["echo", "a b", "it's"])).as_deref(),
            Ok(r"'echo' 'a b' 'it'\''s'")
        );
        assert_eq!(
            command_line(&[]),
            Err("Give the command after --".to_string())
        );
        assert_eq!(
            command_line(&words(&["  "])),
            Err("Give the command after --".to_string())
        );
    }

    #[test]
    fn a_run_prints_the_lines_and_answers_the_exit_code() {
        let (root, scratch, home) = (
            Scratch::new("root"),
            Scratch::new("work"),
            Scratch::new("home"),
        );
        let top = repo(&scratch, "proj");
        keep(&root.0, "ws", &top, &["local"]);
        let ran = |command: &str| {
            let (mut out, mut err) = (Vec::new(), Vec::new());
            let asked = asked_blocking(&root.0, &[], &top, "local");
            let code = run_with(
                asked,
                &[command.to_string()],
                &mut out,
                &mut err,
                |job, line, cancel| build_in(job, &home.0, "laptop", line, cancel),
            );
            (
                code,
                String::from_utf8(out).unwrap(),
                String::from_utf8(err).unwrap(),
            )
        };
        assert_eq!(
            ran("echo hello; echo x > made.txt"),
            (
                0,
                "hello\n".to_string(),
                "onehand: brought back made.txt\n".to_string()
            )
        );
        assert_eq!(ran("exit 3"), (3, String::new(), String::new()));
        assert_eq!(
            ran("nosuchtool"),
            (
                127,
                ran("nosuchtool").1,
                "onehand: nosuchtool is not on local's PATH for ssh\n".to_string()
            )
        );
        // What came back is a file here now, and would go up with the next
        // snapshot: gone here, it is gone there, and so not built.
        std::fs::remove_file(top.join("made.txt")).expect("removed");
        assert_eq!(
            ran("true"),
            (
                1,
                String::new(),
                "onehand: made.txt was not built on local\n".to_string()
            )
        );
    }

    #[test]
    fn a_refusal_is_said_and_exits_one() {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = run_with(
            Err("nope".into()),
            &["true".to_string()],
            &mut out,
            &mut err,
            |_, _, _| panic!("never built"),
        );
        assert_eq!(
            (code, String::from_utf8(err).unwrap()),
            (1, "onehand: nope\n".to_string())
        );
    }
}

#[test]
fn an_entry_written_by_hand_with_a_path_outside_is_refused_before_anything_is_sent() {
    let (home, scratch) = (Scratch::new("home"), Scratch::new("work"));
    let top = repo(&scratch, "proj");
    for (artifacts, into) in [
        (&["../escape"][..], None),
        (&["made.txt"][..], Some(".git")),
    ] {
        let (built, _) = build(
            &home,
            &top,
            "",
            &on_local(artifacts, into),
            "echo x > made.txt",
        );
        assert!(built.is_err(), "{artifacts:?} {into:?}: {built:?}");
    }
    assert!(
        !home.0.join(".onehand-build").exists(),
        "nothing was made on the host"
    );
}

/// The host is asked for the artifacts, but what it sends is its own to
/// choose: only the paths named are written here.
#[test]
fn only_the_paths_named_are_unpacked_whatever_the_host_sends() {
    let scratch = Scratch::new("unpack");
    let (sent, here) = (scratch.0.join("sent"), scratch.0.join("here"));
    write(&sent, "made.txt", "ok\n");
    write(&sent, "dist/page.html", "ok\n");
    write(&sent, ".git/hooks/pre-commit", "#!/bin/sh\necho owned\n");
    write(&sent, "src/main.rs", "overwritten\n");
    std::fs::create_dir_all(&here).expect("here");
    let archive = Command::new("tar")
        .args(["-cf", "-", "-C"])
        .arg(&sent)
        .args(["made.txt", "dist", ".git", "src"])
        .output()
        .expect("tar runs")
        .stdout;
    let mut unpack = unpack(&here, &[PathBuf::from("made.txt"), PathBuf::from("dist")])
        .stdin(std::process::Stdio::piped())
        .spawn()
        .expect("tar runs");
    std::io::Write::write_all(&mut unpack.stdin.take().unwrap(), &archive).expect("fed");
    assert!(unpack.wait().expect("ended").success());
    assert!(here.join("made.txt").exists() && here.join("dist/page.html").exists());
    assert!(!here.join(".git").exists() && !here.join("src").exists());
}

/// Over ssh the line goes to the account's login shell, which may be none a
/// POSIX line means anything to: it is handed to `sh` whole.
#[test]
fn a_script_reaches_a_host_as_one_word_for_sh_and_ssh_gives_up_on_a_dead_host() {
    let addr = HostName("me@build01".into()).addr();
    let cmd = on_host(&addr, "cd 'a b' && echo $HOME");
    let args: Vec<String> = cmd
        .get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        args.last().map(String::as_str),
        Some(r#"sh -c 'cd '\''a b'\'' && echo $HOME'"#)
    );
    assert!(
        args.iter().any(|arg| arg == "ConnectTimeout=10"),
        "{args:?}"
    );
    let before = args
        .iter()
        .position(|arg| arg == "--")
        .expect("options end");
    assert_eq!(args[before + 1], "me@build01");
}

/// The last lines `n` lines leave in a tail.
fn tail_of(n: usize) -> Tail {
    let mut tail = Tail::default();
    for i in 1..=n {
        tail.push(&format!("line {i}"));
    }
    tail
}

#[test]
fn a_tools_answer_says_the_exit_what_came_back_and_the_last_lines() {
    let built = Built {
        exit: Some(0),
        brought: vec!["target/app".into(), "dist".into()],
        hint: None,
    };
    assert_eq!(
        answer(&built, &tail_of(2)),
        "exit 0\nbrought back: target/app, dist\nline 1\nline 2"
    );
    let missing = Built {
        exit: Some(127),
        brought: Vec::new(),
        hint: Some("cargo is not on h's PATH for ssh".into()),
    };
    assert_eq!(
        answer(&missing, &tail_of(0)),
        "exit 127\ncargo is not on h's PATH for ssh"
    );
    let stopped = Built {
        exit: None,
        brought: Vec::new(),
        hint: None,
    };
    assert_eq!(answer(&stopped, &tail_of(0)), "did not finish");
    // Only the last lines are kept, and the answer says how many came before.
    let long = answer(&stopped, &tail_of(TOOL_LINES + 3));
    assert!(
        long.starts_with("did not finish\n… 3 earlier lines not shown\nline 4\n"),
        "{long}"
    );
    assert!(long.ends_with(&format!("line {}", TOOL_LINES + 3)));
}

#[test]
fn a_projects_entry_for_a_host_is_found_or_refused_in_words() {
    use crate::config::{HostOs, HostSpec};
    let builds = [on_local(&["a"], None)];
    assert_eq!(
        super::cli::entry_for("proj", &builds, "local", &[]),
        Ok(builds[0].clone())
    );
    assert_eq!(
        super::cli::entry_for("proj", &[], "local", &[]),
        Err("proj builds on no host; add one on its page".to_string())
    );
    assert_eq!(
        super::cli::entry_for("proj", &builds, "other", &[]),
        Err("proj does not build on other; it builds on local".to_string())
    );
    let windows = [HostSpec {
        name: HostName("local".into()),
        os: HostOs::Windows,
        arch: "AMD64".into(),
    }];
    assert_eq!(
        super::cli::entry_for("proj", &builds, "local", &windows),
        Err("local runs Windows; nothing builds on a Windows host yet".to_string())
    );
}
