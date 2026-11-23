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

fn route_drive_in(n: &str, on: bool) -> RouteDrive {
    let d = std::env::temp_dir().join(format!("onehand-builds-{n}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    let mut drive = RouteDrive::load(&d);
    let _ = drive.set_hosts_on(on);
    drive
}

#[test]
fn routable_hosts_are_linux_ones_from_linux() {
    let mut p = project();
    p.builds.push(BuildHost {
        host: HostName("win".into()),
        artifacts: vec![],
        into: None,
    });
    let declared = [HostSpec {
        name: HostName("win".into()),
        os: HostOs::Windows,
        arch: "x86_64".into(),
    }];
    assert_eq!(
        routable_hosts(&p, &declared, "linux"),
        vec![HostName("100-52".into())]
    );
    assert!(routable_hosts(&p, &declared, "macos").is_empty());
    p.host = Some(HostName("h".into()));
    assert!(routable_hosts(&p, &declared, "linux").is_empty());
}

#[test]
fn builds_says_where_the_next_routed_command_goes() {
    let host = HostName("100-52".into());
    let on = route_drive_in("on", true);
    assert_eq!(
        builds(&project(), &on, &[], "linux"),
        Builds::On(host.clone())
    );
    let mut none = project();
    none.builds_on = None;
    assert_eq!(builds(&none, &on, &[], "linux"), Builds::Here);
    // A pick it cannot route to reads as none.
    assert_eq!(builds(&project(), &on, &[], "macos"), Builds::Here);
    let off = route_drive_in("off", false);
    assert_eq!(builds(&project(), &off, &[], "linux"), Builds::HostsOff);
    let mut down = route_drive_in("down", true);
    down.reached(&host, false);
    assert_eq!(
        builds(&project(), &down, &[], "linux"),
        Builds::NotAnswering(host.clone())
    );
    down.reached(&host, true);
    assert_eq!(builds(&project(), &down, &[], "linux"), Builds::On(host));
}
