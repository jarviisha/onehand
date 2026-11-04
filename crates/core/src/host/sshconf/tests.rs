use std::fs;
use std::path::PathBuf;

use super::*;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("onehand-sshconf-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn with_config(name: &str, text: &str) -> PathBuf {
    let dir = scratch(name);
    fs::write(dir.join("config"), text).unwrap();
    dir
}

#[test]
fn plain_aliases_in_order() {
    let dir = with_config("plain", "Host vps\n  HostName x\nHost box nas\n");
    assert_eq!(aliases(&dir), ["vps", "box", "nas"]);
}

#[test]
fn wildcards_and_negations_are_left_out() {
    let dir = with_config("wild", "Host * !x dev-? ok\n");
    assert_eq!(aliases(&dir), ["ok"]);
}

#[test]
fn include_relative_and_glob() {
    let dir = with_config("inc", "Include conf.d/*\n");
    fs::create_dir(dir.join("conf.d")).unwrap();
    fs::write(dir.join("conf.d/b"), "Host b\n").unwrap();
    fs::write(dir.join("conf.d/a"), "Host a\n").unwrap();
    assert_eq!(aliases(&dir), ["a", "b"]);
}

#[test]
fn include_loop_stops() {
    let dir = with_config("loop", "Host a\nInclude config\n");
    assert_eq!(aliases(&dir), ["a"]);
}

#[test]
fn missing_config_is_empty() {
    assert!(aliases(&scratch("missing")).is_empty());
}

#[test]
fn equals_and_case() {
    let dir = with_config(
        "eq",
        "host=vps\n# Host no\nMatch host z\nHOST  two # tail\n",
    );
    assert_eq!(aliases(&dir), ["vps", "two"]);
}

#[test]
fn duplicates_once() {
    let dir = with_config("dup", "Host a b\nHost b a c\n");
    assert_eq!(aliases(&dir), ["a", "b", "c"]);
}
