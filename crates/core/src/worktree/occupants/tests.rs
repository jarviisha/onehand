use super::*;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

fn at(pid: u32, command: &str, cwd: &str) -> Entry {
    Entry {
        pid,
        command: command.to_string(),
        cwd: Ok(PathBuf::from(cwd)),
    }
}

fn unread(pid: u32, command: &str, kind: ErrorKind) -> Entry {
    Entry {
        pid,
        command: command.to_string(),
        cwd: Err(kind),
    }
}

const W: &str = "/w";

#[test]
fn none_when_every_cwd_is_elsewhere() {
    let table = [at(1, "bash", "/home"), at(2, "node", "/srv")];
    assert_eq!(judge(Path::new(W), table, &[]), Occupants::None);
}

#[test]
fn found_names_pid_and_command() {
    let table = [at(41, "node", "/w/src"), at(2, "bash", "/home")];
    assert_eq!(
        judge(Path::new(W), table, &[]),
        Occupants::Found(vec!["41 node".to_string()])
    );
}

#[test]
fn a_skipped_pid_is_not_found() {
    let table = [at(41, "bash", "/w")];
    assert_eq!(judge(Path::new(W), table, &[41]), Occupants::None);
}

#[test]
fn a_process_gone_meanwhile_is_left_out() {
    let table = [unread(9, "node", ErrorKind::NotFound)];
    assert_eq!(judge(Path::new(W), table, &[]), Occupants::None);
}

#[test]
fn an_unreadable_cwd_is_unknown_never_none() {
    let table = [unread(7, "bash", ErrorKind::PermissionDenied)];
    assert_eq!(
        judge(Path::new(W), table, &[]),
        Occupants::Unknown(vec!["7 bash".to_string()])
    );
}

#[test]
fn found_wins_over_unknown() {
    let table = [
        unread(7, "bash", ErrorKind::PermissionDenied),
        at(41, "node", "/w"),
    ];
    assert_eq!(
        judge(Path::new(W), table, &[]),
        Occupants::Found(vec!["41 node".to_string()])
    );
}

#[test]
fn a_sibling_with_a_common_prefix_is_not_inside() {
    let table = [at(41, "node", "/w2")];
    assert_eq!(judge(Path::new(W), table, &[]), Occupants::None);
}

#[test]
fn parse_lsof_reads_pid_command_and_cwd() {
    let parsed = parse_lsof("p41\ncnode\nn/w/src\np7\ncbash\nn/home\n");
    assert_eq!(
        parsed,
        vec![at(41, "node", "/w/src"), at(7, "bash", "/home")]
    );
}

#[cfg(target_os = "linux")]
#[test]
fn occupants_finds_this_test_process() {
    let here = std::env::current_dir().unwrap();
    match occupants_blocking(&here, &[]) {
        Occupants::Found(found) => {
            let me = format!("{} ", std::process::id());
            assert!(found.iter().any(|line| line.starts_with(&me)), "{found:?}");
        }
        other => panic!("expected this process among {other:?}"),
    }
}
