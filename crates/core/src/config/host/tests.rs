use super::*;
use crate::config::AppConfig;

fn host(name: &str, os: HostOs, arch: &str) -> HostSpec {
    HostSpec {
        name: HostName(name.into()),
        os,
        arch: arch.into(),
    }
}

fn named(name: &str) -> HostName {
    HostName(name.into())
}

#[test]
fn hosts_round_trip() {
    let cfg = AppConfig {
        hosts: vec![
            host("build01", HostOs::Linux, "x86_64"),
            host("win", HostOs::Windows, "AMD64"),
        ],
        ..AppConfig::default()
    };
    let text = cfg.to_toml().expect("serializes");
    assert!(text.contains("[[hosts]]"), "{text}");
    assert_eq!(AppConfig::parse(&text).expect("parses").hosts, cfg.hosts);
}

#[test]
fn an_unknown_os_is_kept_and_runs_no_sessions() {
    let cfg = AppConfig::parse("[[hosts]]\nname = \"x\"\nos = \"plan9\"\narch = \"x86_64\"\n")
        .expect("a system this build does not know still loads");
    assert_eq!(
        cfg.hosts,
        vec![host("x", HostOs::Other("plan9".into()), "x86_64")]
    );
    assert!(!cfg.hosts[0].runs_sessions());
    // Written back as it was read: a save here must not lose a newer build's host.
    assert!(cfg
        .to_toml()
        .expect("serializes")
        .contains("os = \"plan9\""));
}

#[test]
fn only_linux_x86_64_runs_sessions() {
    assert!(host("a", HostOs::Linux, "x86_64").runs_sessions());
    assert!(!host("a", HostOs::Linux, "aarch64").runs_sessions());
    assert!(!host("a", HostOs::Windows, "AMD64").runs_sessions());
    assert!(HostSpec::undeclared(named("a")).runs_sessions());
}

#[test]
fn the_system_reads_as_a_person_names_it() {
    assert_eq!(host("a", HostOs::Linux, "x86_64").system(), "Linux x86_64");
    assert_eq!(
        host("a", HostOs::Windows, "AMD64").system(),
        "Windows AMD64"
    );
    assert_eq!(
        host("a", HostOs::Other("darwin".into()), "arm64").system(),
        "darwin arm64"
    );
}

#[test]
fn put_replaces_by_name_and_remove_drops() {
    let mut hosts = vec![host("a", HostOs::Linux, "x86_64")];
    HostEdit::Put(host("a", HostOs::Windows, "AMD64")).apply(&mut hosts);
    HostEdit::Put(host("b", HostOs::Linux, "x86_64")).apply(&mut hosts);
    assert_eq!(
        hosts,
        vec![
            host("a", HostOs::Windows, "AMD64"),
            host("b", HostOs::Linux, "x86_64")
        ]
    );
    HostEdit::Remove(named("a")).apply(&mut hosts);
    assert_eq!(hosts, vec![host("b", HostOs::Linux, "x86_64")]);
}

#[test]
fn listed_appends_what_projects_name_once() {
    let declared = [host("a", HostOs::Windows, "AMD64")];
    let got = listed(&declared, &[named("b"), named("a"), named("b")]);
    assert_eq!(
        got,
        vec![
            (declared[0].clone(), true),
            (HostSpec::undeclared(named("b")), false)
        ]
    );
}

#[test]
fn removal_is_refused_naming_its_projects() {
    let users = ["api".to_string(), "web".to_string()];
    assert_eq!(
        removal_refusal(&named("build01"), &users),
        Some("build01 is used by api, web".to_string())
    );
    assert_eq!(removal_refusal(&named("build01"), &[]), None);
}

#[test]
fn a_config_with_no_hosts_saves_and_names_none() {
    let text = AppConfig::default().to_toml().expect("serializes");
    assert!(!text.contains("hosts"), "{text}");
}

/// A host written by hand with its name alone is what an undeclared one is
/// taken for, and never costs the rest of the file.
#[test]
fn a_host_written_with_its_name_alone_is_linux_on_x86_64() {
    let cfg = AppConfig::parse("[[hosts]]\nname = \"build01\"\n").expect("still loads");
    assert_eq!(cfg.hosts, vec![HostSpec::undeclared(named("build01"))]);
}

#[test]
fn a_system_is_read_whatever_its_case() {
    let cfg = AppConfig::parse("[[hosts]]\nname = \"a\"\nos = \"Linux\"\narch = \"x86_64\"\n")
        .expect("loads");
    assert_eq!(cfg.hosts[0].os, HostOs::Linux);
    assert!(cfg.hosts[0].runs_sessions());
    assert!(cfg
        .to_toml()
        .expect("serializes")
        .contains("os = \"linux\""));
}

#[test]
fn adding_refuses_no_name_and_a_declared_one() {
    let hosts = [host("build01", HostOs::Linux, "x86_64")];
    assert_eq!(add_refusal("  ", &hosts).as_deref(), Some("Name the host"));
    assert_eq!(
        add_refusal(" build01 ", &hosts).as_deref(),
        Some("build01 is already declared")
    );
    assert_eq!(add_refusal("other", &hosts), None);
}

#[test]
fn a_session_is_refused_on_a_host_declared_as_one_that_only_builds() {
    let hosts = [
        host("win", HostOs::Windows, "AMD64"),
        host("build01", HostOs::Linux, "x86_64"),
    ];
    assert_eq!(
        builds_only(" win ", &hosts).as_deref(),
        Some("win only builds; it runs no sessions")
    );
    assert_eq!(builds_only("build01", &hosts), None);
    assert_eq!(builds_only("never-declared", &hosts), None);
}

#[test]
fn a_declared_host_is_found_by_its_name() {
    let hosts = [host("build01", HostOs::Linux, "x86_64")];
    assert!(declared(&hosts, " build01 ").is_some());
    assert!(declared(&hosts, "other").is_none());
}
