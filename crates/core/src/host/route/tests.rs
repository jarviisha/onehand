use super::*;
use crate::config::HostOs;
use crate::project::{Project, ProjectId};
use crate::workspace::{Offloaded, RoutedCommand};

fn project() -> Project {
    let mut p = Project::new(ProjectId::mint(1), None, "/p".into());
    p.builds = vec![BuildHost {
        host: HostName("100-52".into()),
        artifacts: vec![],
        into: Some("x".into()),
    }];
    p.builds_on = Some(HostName("100-52".into()));
    p.routes = vec![RoutedCommand::parse("cargo build", "target/debug/x").unwrap()];
    p
}

fn found(p: Project) -> Result<Found, String> {
    Ok(Found {
        project: p,
        top: "/p".into(),
        within: "crates/a".into(),
    })
}

fn ask(args: &[&str]) -> Ask {
    Ask {
        program: "cargo".into(),
        args: args.iter().map(|a| a.to_string()).collect(),
        cwd: "/p/crates/a".into(),
    }
}

fn here(found: Result<Found, String>) -> bool {
    matches!(
        decide(found, true, "linux", &[], &ask(&["build"])),
        Decision::Here
    )
}

#[test]
fn here_without_a_project() {
    assert!(here(Err("no".into())));
}

#[test]
fn here_without_a_pick() {
    let mut p = project();
    p.builds_on = None;
    assert!(here(found(p)));
}

#[test]
fn here_when_hosts_are_off() {
    let d = decide(found(project()), false, "linux", &[], &ask(&["build"]));
    assert!(matches!(d, Decision::Here));
}

#[test]
fn here_for_a_host_project() {
    let mut p = project();
    p.host = Some(HostName("h".into()));
    assert!(here(found(p)));
}

#[test]
fn here_for_an_offloaded_project() {
    let mut p = project();
    p.offload = Some(Offloaded {
        host: HostName("h".into()),
        mirror: "/m".into(),
        branch: None,
    });
    assert!(here(found(p)));
}

#[test]
fn here_when_the_pick_is_no_longer_a_build_host() {
    let mut p = project();
    p.builds.clear();
    assert!(here(found(p)));
}

#[test]
fn here_when_no_route_matches() {
    let d = decide(found(project()), true, "linux", &[], &ask(&["test"]));
    assert!(matches!(d, Decision::Here));
}

#[test]
fn here_for_a_windows_host() {
    let declared = [HostSpec {
        name: HostName("100-52".into()),
        os: HostOs::Windows,
        arch: "x86_64".into(),
    }];
    let d = decide(found(project()), true, "linux", &declared, &ask(&["build"]));
    assert!(matches!(d, Decision::Here));
}

#[test]
fn here_from_a_mac() {
    let d = decide(found(project()), true, "macos", &[], &ask(&["build"]));
    assert!(matches!(d, Decision::Here));
}

#[test]
fn host_for_a_matching_route() {
    let d = decide(
        found(project()),
        true,
        "linux",
        &[],
        &ask(&["build", "-p", "x"]),
    );
    let Decision::Host(r) = d else { panic!("host") };
    assert_eq!(r.host.0, "100-52");
    assert_eq!(r.entry.artifacts, vec![PathBuf::from("target/debug/x")]);
    assert_eq!(r.entry.into, None);
    assert_eq!(r.command, "'cargo' 'build' '-p' 'x'");
    assert_eq!(r.within, PathBuf::from("crates/a"));
}

#[test]
fn args_keep_their_words() {
    let args = ["build", "--", "a b", "it's"];
    let Decision::Host(r) = decide(found(project()), true, "linux", &[], &ask(&args)) else {
        panic!("host")
    };
    let mut words = vec!["cargo".to_string()];
    words.extend(args.iter().map(|a| a.to_string()));
    assert_eq!(r.command, command_line(&words, &HostOs::Linux).unwrap());
}
