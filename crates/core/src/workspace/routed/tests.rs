use super::*;
use crate::project::file::{load_from, save_to, ProjectLoad, FILE};
use crate::project::{Project, ProjectId};
use crate::workspace::HostName;

fn args(words: &[&str]) -> Vec<String> {
    words.iter().map(|w| w.to_string()).collect()
}

#[test]
fn parse_keeps_words_and_artifacts() {
    let r = RoutedCommand::parse(
        "  cargo   build ",
        "target/debug/onehand\n\ntarget/release/onehand",
    )
    .unwrap();
    assert_eq!(r.words, "cargo build");
    assert_eq!(r.program(), "cargo");
    assert_eq!(r.artifacts.len(), 2);
}

#[test]
fn program_word_is_a_plain_name() {
    for bad in [
        "../cargo build",
        "/usr/bin/cargo build",
        "car'go build",
        "",
        "   ",
    ] {
        assert!(RoutedCommand::parse(bad, "").is_err(), "{bad}");
    }
    for ok in ["cargo-nextest run", "dotnet build"] {
        assert!(RoutedCommand::parse(ok, "").is_ok(), "{ok}");
    }
}

#[test]
fn artifacts_are_refused_as_build_hosts_are() {
    assert!(RoutedCommand::parse("cargo build", "../x").is_err());
}

#[test]
fn matches_leading_words_after_options() {
    let r = RoutedCommand::parse("cargo build", "").unwrap();
    assert!(r.matches("cargo", &args(&["build", "-p", "x"])));
    assert!(r.matches("cargo", &args(&["+nightly", "build"])));
    assert!(r.matches("cargo", &args(&["-q", "build", "--release"])));
    assert!(!r.matches("cargo", &args(&["test"])));
    assert!(!r.matches("cargo", &args(&["builds"])));
    assert!(!r.matches("dotnet", &args(&["build"])));
}

#[test]
fn matches_a_bare_program() {
    let r = RoutedCommand::parse("make", "").unwrap();
    assert!(r.matches("make", &[]));
    assert!(r.matches("make", &args(&["all"])));
}

#[test]
fn project_file_round_trips_routes_and_pick() {
    let dir = crate::project::testing::scratch("routes");
    let mut p = Project::new(ProjectId::mint(3), None, "/a".into());
    p.routes = vec![
        RoutedCommand::parse("cargo build", "target/debug/x").unwrap(),
        RoutedCommand::parse("make", "").unwrap(),
    ];
    p.builds_on = Some(HostName("100-52".into()));
    save_to(&p, &dir).unwrap();
    assert!(matches!(load_from(&dir), ProjectLoad::Found(back) if *back == p));

    let plain = Project::new(ProjectId::mint(4), None, "/b".into());
    save_to(&plain, &dir).unwrap();
    let text = std::fs::read_to_string(dir.join(FILE)).unwrap();
    assert!(
        !text.contains("routes") && !text.contains("builds_on"),
        "{text}"
    );
}
