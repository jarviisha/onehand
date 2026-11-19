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
