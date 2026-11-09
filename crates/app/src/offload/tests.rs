//! The offloaded projects' carrier against its contract: a job's answer
//! reaches the drive once, the poll's clock comes round and is armed again,
//! an answer reaches whoever asked once, and a late answer for a folder no
//! longer kept lands on nothing. The jobs here touch no host: a host's
//! attach runs on a runtime the test scheduler does not drive.

use crate::state::Shared;
use gpui::TestAppContext;
use onehand_core::host::offload::branch::tracking;
use onehand_core::host::offload::drive::OFFLOAD_POLL;
use onehand_core::workspace::HostName;
use std::path::{Path, PathBuf};
use std::process::Command;

fn app(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(|cx| cx.set_global(Shared::for_test()));
}

/// A folder of its own, named for the test, that nothing kept anything of.
fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "onehand-offload-carrier-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .current_dir(dir)
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {out:?}");
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

/// A repository with one commit, and the host's branch fetched at it.
fn repo(name: &str, branch: &str) -> PathBuf {
    let dir = folder(name);
    git(&dir, &["init", "-q"]);
    git(&dir, &["commit", "-q", "--allow-empty", "-m", "one"]);
    git(&dir, &["update-ref", &tracking(branch), "HEAD"]);
    dir
}

fn ahead(cx: &mut TestAppContext, path: &Path) -> Option<usize> {
    cx.update(|cx| super::get(cx, path)?.branch.as_ref()?.ahead)
}

/// What was kept of a folder is read once, off the UI loop, and lands on
/// its entry: nothing kept is this machine's code.
#[gpui::test]
fn what_was_kept_is_read_and_lands_once(cx: &mut TestAppContext) {
    app(cx);
    let path = folder("read");
    cx.update(|cx| super::track(cx, path.clone(), HostName("vps".into()), None));
    assert!(cx.update(|cx| super::get(cx, &path).is_some_and(|o| o.locked())));
    cx.run_until_parked();
    assert!(cx.update(|cx| super::holds_code(cx, &path)));
    let _ = std::fs::remove_dir_all(&path);
}

/// A branch is counted at once, and again each time the poll's clock comes
/// round, which it arms again after every pass.
#[gpui::test]
fn the_polls_clock_comes_round_and_is_armed_again(cx: &mut TestAppContext) {
    app(cx);
    let branch = "onehand/vps";
    let path = repo("poll", branch);
    cx.update(|cx| super::track_branch(cx, path.clone(), HostName("vps".into()), branch.into()));
    cx.run_until_parked();
    assert_eq!(ahead(cx, &path), Some(0));
    for want in 1..=2 {
        // The host's branch moved on by a commit, as a fetch would bring it.
        let tip = git(&path, &["rev-parse", &tracking(branch)]);
        let next = git(
            &path,
            &[
                "commit-tree",
                "-p",
                &tip,
                "-m",
                "more",
                &format!("{tip}^{{tree}}"),
            ],
        );
        git(&path, &["update-ref", &tracking(branch), &next]);
        cx.executor().advance_clock(OFFLOAD_POLL);
        cx.run_until_parked();
        assert_eq!(ahead(cx, &path), Some(want), "pass {want}");
    }
    let _ = std::fs::remove_dir_all(&path);
}

/// An answer reaches the one who asked, once: a take that cannot go now is
/// refused at once, each asker hearing its own.
#[gpui::test]
fn an_answer_reaches_its_asker_once(cx: &mut TestAppContext) {
    app(cx);
    let path = folder("ask");
    cx.update(|cx| super::track(cx, path.clone(), HostName("vps".into()), None));
    cx.run_until_parked();
    let (first, second) = cx.update(|cx| {
        (
            super::take_over(cx, path.clone(), path.join("backup")),
            super::take_over(cx, path.clone(), path.join("backup")),
        )
    });
    cx.run_until_parked();
    for took in [first, second] {
        let said = futures::FutureExt::now_or_never(took).expect("answered");
        assert_eq!(
            said,
            Err("The code was moving; check again, then take it".into())
        );
    }
    let page = cx.update(|cx| super::get(cx, &path).and_then(|o| o.failed.clone()));
    assert!(page.is_some(), "said on the project's page too");
    // A send with nothing of this machine's to send is refused, not held.
    let path_b = folder("nothing");
    assert!(cx.update(|cx| super::send_up(cx, &path_b).is_none()));
    let _ = std::fs::remove_dir_all(&path);
    let _ = std::fs::remove_dir_all(&path_b);
}

/// A folder forgotten while its job runs: the answer lands on nothing.
#[gpui::test]
fn a_late_answer_for_a_folder_let_go_lands_on_nothing(cx: &mut TestAppContext) {
    app(cx);
    let branch = "onehand/vps";
    let path = repo("late", branch);
    cx.update(|cx| {
        super::track_branch(cx, path.clone(), HostName("vps".into()), branch.into());
        super::forget(cx, &path);
    });
    cx.run_until_parked();
    assert!(cx.update(|cx| super::get(cx, &path).is_none()));
    let _ = std::fs::remove_dir_all(&path);
}
