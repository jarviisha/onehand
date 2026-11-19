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
        entry: host,
        os: crate::config::HostOs::Linux,
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
            .arg(format!(
                "printf %s {}",
                shell::Shell::Posix.word(text).expect("any text")
            ))
            .output()
            .expect("sh runs");
        assert_eq!(String::from_utf8_lossy(&out.stdout), text);
    }
}

#[test]
fn the_laptops_id_is_kept_to_safe_letters_in_the_folders_name() {
    let dir = host_dir("a b&c|d", Path::new("/work/proj"));
    assert!(dir.starts_with(".onehand-build/a_b_c_d/proj-"), "{dir}");
}

#[test]
fn what_a_host_said_of_a_program_it_lacks_is_kept_whole() {
    let two = b"'git' is not recognized as an internal or external command,\r\noperable program or batch file.\r\n";
    assert_eq!(
        said(two),
        "'git' is not recognized as an internal or external command, operable program or batch file."
    );
    assert_eq!(said(b"warning: x\nfatal: no\n"), "fatal: no");
}

#[test]
fn the_folder_on_the_host_is_named_safely_and_by_the_laptop() {
    let dir = host_dir("lap-1", Path::new("/work/my proj's"));
    assert!(dir.starts_with(".onehand-build/lap-1/my_proj_s-"), "{dir}");
    assert_ne!(dir, host_dir("lap-1", Path::new("/other/my proj's")));
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
            entry: &host,
            os: crate::config::HostOs::Linux,
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
    use crate::project::{Project, ProjectId};

    /// A config root keeping `top` as a project building on `hosts`.
    fn keep(root: &Path, top: &Path, hosts: &[&str]) {
        let seed = u64::from(crate::workspace::fnv1a(top.as_os_str().as_encoded_bytes()));
        let mut project = Project::new(ProjectId::mint(seed), None, top.to_path_buf());
        project.builds = hosts
            .iter()
            .map(|host| BuildHost {
                host: HostName((*host).into()),
                artifacts: vec!["made.txt".into()],
                into: None,
            })
            .collect();
        let dir = crate::config::projects_in(root).join(project.id.as_str());
        crate::project::file::save_to(&project, &dir).expect("saved");
    }

    #[test]
    fn a_project_is_found_from_a_folder_inside_it() {
        let (root, scratch) = (Scratch::new("root"), Scratch::new("work"));
        let top = repo(&scratch, "proj");
        write(&top, "crates/app/x.rs", "");
        keep(&root.0, &top, &["build01"]);
        let asked =
            asked_blocking(&root.0, &[], &top.join("crates/app"), "build01").expect("found");
        assert_eq!(asked.top, top);
        assert_eq!(asked.within, PathBuf::from("crates/app"));
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
        keep(&root.0, &top, &["build01"]);
        let asked = asked_blocking(&root.0, &[], &tree, "build01").expect("found");
        assert_eq!(asked.top, tree);
        assert_eq!(asked.within, PathBuf::new());
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
            format!("{} is not a project onehand keeps", stray.display())
        );
        let bare = repo(&scratch, "bare");
        keep(&root.0, &bare, &[]);
        assert_eq!(
            refused(&[], &bare, "h"),
            "bare builds on no host; add one on its page"
        );
        let top = repo(&scratch, "proj");
        keep(&root.0, &top, &["build01", "win"]);
        assert_eq!(
            refused(&[], &top, "other"),
            "proj does not build on other; it builds on build01, win"
        );
        let other = [HostSpec {
            name: HostName("win".into()),
            os: HostOs::Other("plan9".into()),
            arch: "x86_64".into(),
        }];
        assert_eq!(
            refused(&other, &top, "win"),
            "win runs plan9; nothing builds on it yet"
        );
        let windows = [HostSpec {
            name: HostName("win".into()),
            os: HostOs::Windows,
            arch: "AMD64".into(),
        }];
        let asked = asked_blocking(&root.0, &windows, &top, "win").expect("a Windows host builds");
        assert_eq!(asked.os, HostOs::Windows);
    }

    #[test]
    fn a_worktree_anywhere_is_found_through_its_main_checkout() {
        let (root, scratch) = (Scratch::new("root"), Scratch::new("work"));
        let top = repo(&scratch, "proj");
        let tree = scratch.0.join("elsewhere-feature");
        git(
            &top,
            &["worktree", "add", "-q", "-b", "f", tree.to_str().unwrap()],
        );
        keep(&root.0, &top, &["build01"]);
        let asked = asked_blocking(&root.0, &[], &tree, "build01").expect("found");
        assert_eq!(asked.entry.host, HostName("build01".into()));
    }

    #[test]
    fn command_line_keeps_one_word_and_quotes_several() {
        let words =
            |words: &[&str]| -> Vec<String> { words.iter().map(|w| w.to_string()).collect() };
        assert_eq!(
            command_line(&words(&["cargo test && ls"]), &HostOs::Linux).as_deref(),
            Ok("cargo test && ls")
        );
        assert_eq!(
            command_line(&words(&["echo", "a b", "it's"]), &HostOs::Linux).as_deref(),
            Ok(r"'echo' 'a b' 'it'\''s'")
        );
        assert_eq!(
            command_line(&[], &HostOs::Linux),
            Err("Give the command after --".to_string())
        );
        assert_eq!(
            command_line(&words(&["  "]), &HostOs::Linux),
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
        keep(&root.0, &top, &["local"]);
        let ran = |command: &str| {
            let (mut out, mut err) = (Vec::new(), Vec::new());
            let asked = asked_blocking(&root.0, &[], &top, "local");
            let code = run_with(
                asked,
                &[command.to_string()],
                &mut out,
                &mut err,
                |job, line| build_in(job, &home.0, "laptop", line, &AtomicBool::new(false)),
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
    fn several_words_for_a_windows_host_are_joined_for_cmd() {
        let words: Vec<String> = ["dotnet", "build", "my app.sln"]
            .iter()
            .map(|w| w.to_string())
            .collect();
        assert_eq!(
            command_line(&words, &HostOs::Windows).as_deref(),
            Ok("dotnet build \"my app.sln\"")
        );
        // One word is the person's own line, for whichever shell.
        assert_eq!(
            command_line(&["dotnet build && dir".to_string()], &HostOs::Windows).as_deref(),
            Ok("dotnet build && dir")
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
            |_, _| panic!("never built"),
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
    let cmd = on_host(&addr, shell::Shell::Posix, "cd 'a b' && echo $HOME");
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
        Ok((builds[0].clone(), HostOs::Linux))
    );
    assert_eq!(
        super::cli::entry_for("proj", &[], "local", &[]),
        Err("proj builds on no host; add one on its page".to_string())
    );
    assert_eq!(
        super::cli::entry_for("proj", &builds, "other", &[]),
        Err("proj does not build on other; it builds on local".to_string())
    );
    // A Windows host builds, and is asked in its own shell's words.
    let windows = [HostSpec {
        name: HostName("local".into()),
        os: HostOs::Windows,
        arch: "AMD64".into(),
    }];
    assert_eq!(
        super::cli::entry_for("proj", &builds, "local", &windows),
        Ok((builds[0].clone(), HostOs::Windows))
    );
    // A system this build does not know is still refused.
    let other = [HostSpec {
        name: HostName("local".into()),
        os: HostOs::Other("plan9".into()),
        arch: "x86_64".into(),
    }];
    assert_eq!(
        super::cli::entry_for("proj", &builds, "local", &other),
        Err("local runs plan9; nothing builds on it yet".to_string())
    );
}

mod shells {
    use super::super::shell::Shell;
    use crate::config::HostOs;
    use std::path::{Path, PathBuf};

    const SHA: &str = "2ab98d7259af09c62a806ff487818d931abf3b7e";
    const DIR: &str = ".onehand-build/lap/my_proj-abc";

    #[test]
    fn a_windows_host_is_told_in_cmds_words_and_every_other_in_posix() {
        assert_eq!(Shell::of(&HostOs::Windows), Shell::Cmd);
        assert_eq!(Shell::of(&HostOs::Linux), Shell::Posix);
        assert_eq!(Shell::of(&HostOs::Other("darwin".into())), Shell::Posix);
    }

    #[test]
    fn cmd_words_are_double_quoted() {
        assert_eq!(
            Shell::Cmd.word("out/made file.txt").as_deref(),
            Ok("\"out/made file.txt\"")
        );
    }

    #[test]
    fn cmd_refuses_a_percent_or_a_quote() {
        for text in ["100%/out", "a\"b"] {
            assert_eq!(
                Shell::Cmd.word(text),
                Err(format!(
                    "{text} cannot be built on a Windows host: its name holds % or \""
                ))
            );
        }
        // And so does every line built from such a word.
        assert!(Shell::Cmd.run(DIR, SHA, Path::new("50%"), "dir").is_err());
        assert!(Shell::Cmd.send(DIR, &[PathBuf::from("a\"b")]).is_err());
        assert_eq!(Shell::Posix.word("100%").as_deref(), Ok("'100%'"));
    }

    #[test]
    fn cmd_init_makes_the_repository_and_keeps_the_code_as_it_is_here() {
        assert_eq!(
            Shell::Cmd.init(DIR).as_deref(),
            Ok("(where git >nul 2>nul || (echo git>&2 & exit 4)) && (where tar >nul 2>nul || (echo tar>&2 & exit 4)) && git init -q \".onehand-build/lap/my_proj-abc\" && git -C \".onehand-build/lap/my_proj-abc\" config core.autocrlf false && git -C \".onehand-build/lap/my_proj-abc\" config core.longpaths true")
        );
    }

    #[test]
    fn cmd_run_is_the_line_checked_by_hand() {
        assert_eq!(
            Shell::Cmd.run(DIR, SHA, Path::new("sub dir"), "dotnet build && dir").as_deref(),
            Ok("cd /d \".onehand-build/lap/my_proj-abc\" && git checkout -q -f --detach 2ab98d7259af09c62a806ff487818d931abf3b7e && cd \"sub dir\" && echo __ONEHAND_BUILD__ && cmd /d /s /c ^\"dotnet build ^&^& dir^\" 2>&1")
        );
        let top = Shell::Cmd
            .run(DIR, SHA, Path::new(""), "dir")
            .expect("a line");
        assert!(!top.contains("cd \"\""), "{top}");
    }

    /// The line is read twice, by the `cmd.exe` ssh starts and by the one
    /// that runs the command: every character the first would act on is
    /// escaped, so the second is handed the person's line as they wrote it.
    #[test]
    fn a_cmd_command_reaches_its_own_shell_as_written() {
        let line = Shell::Cmd
            .run(
                DIR,
                SHA,
                Path::new(""),
                r#"findstr "a|b" f.txt & echo (x) > o^ut"#,
            )
            .expect("a line");
        assert!(
            line.ends_with(
                r#"cmd /d /s /c ^"findstr ^"a^|b^" f.txt ^& echo ^(x^) ^> o^^ut^" 2>&1"#
            ),
            "{line}"
        );
    }

    #[test]
    fn cmd_refuses_a_line_break_in_a_word() {
        for text in ["a\nb", "a\rb", "a\u{0}b"] {
            assert_eq!(
                Shell::Cmd.word(text),
                Err(format!(
                    "{} cannot be built on a Windows host: its name holds a line break",
                    text.escape_debug()
                ))
            );
        }
    }

    #[test]
    fn cmd_send_checks_then_tars() {
        let paths = [PathBuf::from("out/made file.txt"), PathBuf::from("out/dir")];
        assert_eq!(
            Shell::Cmd.send(DIR, &paths).as_deref(),
            Ok("cd /d \".onehand-build/lap/my_proj-abc\" && (for %p in (\"out/made file.txt\" \"out/dir\") do @if not exist %p (echo %~p>&2 & exit 3)) && tar -cf - -- \"out/made file.txt\" \"out/dir\"")
        );
    }

    #[test]
    fn cmd_goes_over_ssh_as_it_is_and_posix_through_sh() {
        assert_eq!(Shell::Cmd.over_ssh("cd /d \"a b\""), "cd /d \"a b\"");
        assert_eq!(Shell::Posix.over_ssh("cd 'a b'"), r"sh -c 'cd '\''a b'\'''");
    }

    /// The program that was not there is read from what the shell said of
    /// it, since the command's first word may be `cd`, a variable, or a tool
    /// that ran another; only when that cannot be read is it the first word.
    #[test]
    fn a_program_that_is_not_there_is_named_from_what_the_shell_said() {
        let lines = |said: &[&str]| -> Vec<String> { said.iter().map(|l| l.to_string()).collect() };
        let posix =
            |said: &[&str], command: &str| Shell::Posix.not_found(Some(127), &lines(said), command);
        assert_eq!(
            posix(
                &["/bin/bash: line 1: cargo: command not found"],
                "cd sub && cargo build"
            )
            .as_deref(),
            Some("cargo")
        );
        assert_eq!(
            posix(&["sh: 1: nosuchtool: not found"], "FOO=1 nosuchtool").as_deref(),
            Some("nosuchtool")
        );
        assert_eq!(
            posix(&["zsh:1: command not found: nosuchtool"], "x").as_deref(),
            Some("nosuchtool")
        );
        // Said in no words this knows: the command's first word, unquoted.
        assert_eq!(
            posix(&["something else"], "'mytool' '--v'").as_deref(),
            Some("mytool")
        );
        assert_eq!(
            Shell::Posix.not_found(Some(1), &lines(&["x: not found"]), "x"),
            None
        );

        let sentence = [
            "'cargo' is not recognized as an internal or external command,",
            "operable program or batch file.",
        ];
        assert_eq!(
            Shell::Cmd
                .not_found(Some(1), &lines(&sentence), "cd sub && cargo build")
                .as_deref(),
            Some("cargo")
        );
        assert_eq!(
            Shell::Cmd.not_found(
                Some(1),
                &lines(&["error CS1002: ; expected"]),
                "dotnet build"
            ),
            None
        );
        assert_eq!(Shell::Cmd.not_found(Some(0), &lines(&sentence), "x"), None);
    }

    /// A host with no git or no tar is refused at the first step, before
    /// any code is sent, naming what it lacks.
    #[test]
    fn a_host_without_what_a_build_needs_is_refused_naming_it() {
        use super::super::needs_refusal;
        use crate::workspace::HostName;
        let host = HostName("build01".into());
        assert_eq!(
            needs_refusal(&host, Some(4), b" git tar"),
            Some("build01 has no git or tar; a build needs both".to_string())
        );
        assert_eq!(
            needs_refusal(&host, Some(4), b"tar \r\n"),
            Some("build01 has no tar; a build needs git and tar".to_string())
        );
        assert_eq!(needs_refusal(&host, Some(1), b"git"), None);
    }

    #[test]
    fn several_words_are_joined_for_each_shell() {
        let words: Vec<String> = ["dotnet", "build", "my app.sln", "-c", "Release", "a&b"]
            .iter()
            .map(|w| w.to_string())
            .collect();
        assert_eq!(
            Shell::Cmd.join(&words).as_deref(),
            Ok("dotnet build \"my app.sln\" -c Release \"a&b\"")
        );
        assert_eq!(
            Shell::Posix.join(&words).as_deref(),
            Ok("'dotnet' 'build' 'my app.sln' '-c' 'Release' 'a&b'")
        );
        // A quoted word ending in a backslash would read as an escaped quote
        // to the program it is handed to: the backslash is doubled.
        let folder = ["dir".to_string(), r"C:\my out\".to_string()];
        assert_eq!(
            Shell::Cmd.join(&folder).as_deref(),
            Ok(r#"dir "C:\my out\\""#)
        );
        // A word holding a quote cannot be joined for cmd.exe.
        let quoted = ["echo".to_string(), "say \"hi\"".to_string()];
        assert_eq!(
            Shell::Cmd.join(&quoted),
            Err(
                "say \"hi\" holds a quote; give the whole command as one word after --".to_string()
            )
        );
    }

    #[test]
    fn posix_scripts_are_what_they_were() {
        assert_eq!(
            Shell::Posix.init(DIR).as_deref(),
            Ok(
                r#"m=; for t in git tar; do command -v $t >/dev/null 2>&1 || m="$m $t"; done; test -z "$m" || { printf %s "$m" >&2; exit 4; }; mkdir -p '.onehand-build/lap/my_proj-abc' && git -C '.onehand-build/lap/my_proj-abc' init -q"#
            )
        );
        assert_eq!(
            Shell::Posix
                .run(DIR, SHA, Path::new("sub dir"), "it's")
                .as_deref(),
            Ok(
                r#"cd '.onehand-build/lap/my_proj-abc' && git checkout -q -f --detach 2ab98d7259af09c62a806ff487818d931abf3b7e && cd 'sub dir' && echo __ONEHAND_BUILD__ && exec "${SHELL:-/bin/sh}" -lc 'it'\''s' 2>&1"#
            )
        );
        assert_eq!(
            Shell::Posix.send(DIR, &[PathBuf::from("a b")]).as_deref(),
            Ok(
                r#"cd '.onehand-build/lap/my_proj-abc' && for p in 'a b'; do test -e "$p" || { printf %s "$p" >&2; exit 3; }; done && tar -cf - -- 'a b'"#
            )
        );
    }
}
