use super::*;
use crate::workspace::HostName;

const WINDOWS_TOOLS: &str = "__ONEHAND_PROBE__\nAMD64\ngit=\ngit version 2.56.1.windows.1\ntar=\nbsdtar 3.5.2 - libarchive 3.5.2 zlib/1.2.5.f-ipp \ndotnet=\n8.0.405\n";

#[test]
fn linux_is_read_past_the_startup_noise() {
    assert_eq!(
        system("Welcome!\n__ONEHAND_PROBE__\nLinux x86_64\n"),
        Some((HostOs::Linux, "x86_64".to_string()))
    );
}

#[test]
fn windows_is_read_from_ver() {
    assert_eq!(
        system("__ONEHAND_PROBE__ \n\nMicrosoft Windows [Version 10.0.17763.8027]\n"),
        Some((HostOs::Windows, String::new()))
    );
}

#[test]
fn git_bash_is_windows() {
    assert_eq!(
        system("__ONEHAND_PROBE__\nMINGW64_NT-10.0-17763 x86_64\n").map(|(os, _)| os),
        Some(HostOs::Windows)
    );
}

#[test]
fn another_system_is_kept_as_it_named_itself() {
    assert_eq!(
        system("__ONEHAND_PROBE__\nDarwin arm64\n"),
        Some((HostOs::Other("Darwin".to_string()), "arm64".to_string()))
    );
}

#[test]
fn no_marker_is_no_answer() {
    assert_eq!(system("Linux x86_64\n"), None);
    assert_eq!(system("__ONEHAND_PROBE__\n"), None);
}

#[test]
fn windows_tools() {
    let (arch, tools) = tools(WINDOWS_TOOLS, true).expect("answered");
    assert_eq!(arch.as_deref(), Some("AMD64"));
    let probed = Probed {
        os: HostOs::Windows,
        arch: arch.unwrap(),
        tools,
    };
    assert_eq!(probed.tools.len(), 3);
    assert_eq!(probed.tools[2].name, "dotnet");
    assert_eq!(probed.tools[2].said, "8.0.405");
    assert!(probed.missing().is_empty());
    assert!(probed.has("git") && !probed.has("cargo"));
}

#[test]
fn linux_tools_without_git() {
    let (arch, tools) = tools(
        "noise\n__ONEHAND_PROBE__\ntar=\ntar (GNU tar) 1.35\n",
        false,
    )
    .expect("answered");
    assert_eq!(arch, None);
    let probed = Probed {
        os: HostOs::Linux,
        arch: "x86_64".into(),
        tools,
    };
    assert_eq!(probed.missing(), vec!["git"]);
}

#[test]
fn a_tool_that_says_nothing_of_its_version_is_still_found() {
    let (_, tools) = tools("__ONEHAND_PROBE__\ngit=\ntar=\ntar 1.35\n", false).expect("answered");
    let said: Vec<(&str, &str)> = tools
        .iter()
        .map(|tool| (tool.name.as_str(), tool.said.as_str()))
        .collect();
    assert_eq!(said, vec![("git", ""), ("tar", "tar 1.35")]);
}

#[test]
fn probes_this_machine() {
    let probed = probe_blocking(&HostName("local".into()).addr()).expect("this machine answers");
    assert_eq!(probed.os, HostOs::Linux);
    assert!(!probed.arch.is_empty());
    assert!(probed.has("tar"));
}

/// A shell that could not run the second call says nothing after the mark:
/// that is a failed probe, never a host with nothing installed.
#[test]
fn a_second_call_that_never_answered_is_no_answer() {
    assert_eq!(tools("fish: Unsupported use of '='\n", false), None);
    assert_eq!(tools("", true), None);
}

/// A Windows host whose ssh shell is Git's bash is asked in POSIX words,
/// and one answering through cmd.exe in cmd.exe's.
#[test]
fn each_shell_is_asked_in_its_own_words() {
    assert!(asks_cmd(&HostOs::Windows, ""));
    assert!(!asks_cmd(&HostOs::Windows, "x86_64"));
    assert!(!asks_cmd(&HostOs::Linux, "x86_64"));
}

#[test]
fn a_host_that_never_finishes_answering_is_given_up_on() {
    let local = HostName("local".into()).addr();
    let started = std::time::Instant::now();
    let said = run(&local, "sleep 30", std::time::Duration::from_millis(200));
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
    assert_eq!(said, Err("local did not answer within 0s".to_string()));
}
