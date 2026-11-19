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
    let (arch, tools) = tools(WINDOWS_TOOLS, true);
    assert_eq!(arch.as_deref(), Some("AMD64"));
    let probed = Probed {
        os: HostOs::Windows,
        arch: arch.unwrap(),
        tools,
    };
    assert_eq!(probed.tools.len(), 3);
    assert_eq!(
        probed.tools[2],
        ("dotnet".to_string(), "8.0.405".to_string())
    );
    assert!(probed.missing().is_empty());
    assert!(probed.has("git") && !probed.has("cargo"));
}

#[test]
fn linux_tools_without_git() {
    let (arch, tools) = tools(
        "noise\n__ONEHAND_PROBE__\ntar=\ntar (GNU tar) 1.35\n",
        false,
    );
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
    let (_, tools) = tools("__ONEHAND_PROBE__\ngit=\ntar=\ntar 1.35\n", false);
    assert_eq!(
        tools,
        vec![
            ("git".to_string(), String::new()),
            ("tar".to_string(), "tar 1.35".to_string())
        ]
    );
}

#[test]
fn probes_this_machine() {
    let probed = probe_blocking(&HostName("local".into()).addr()).expect("this machine answers");
    assert_eq!(probed.os, HostOs::Linux);
    assert!(!probed.arch.is_empty());
    assert!(probed.has("tar"));
}
