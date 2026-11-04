use super::*;

/// A scratch directory for an install, run here through `sh`.
fn install_dir(name: &str) -> String {
    let dir = std::env::temp_dir().join(format!("onehand-install-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir.display().to_string()
}

const HERE: HostAddr = HostAddr {
    ssh: String::new(),
    local: true,
};

#[tokio::test]
async fn a_copy_that_names_its_build_is_moved_into_place() {
    let dir = install_dir("good");
    let script = b"#!/bin/sh\necho abc\n";
    run(&HERE, &copy_command(&dir, "abc"), script)
        .await
        .unwrap();
    let path = Path::new(&dir).join("onehand-host");
    assert_eq!(std::fs::read(&path).unwrap(), script);
    assert!(!Path::new(&dir).join("onehand-host.tmp").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn a_copy_that_warns_as_it_loads_and_names_its_build_is_installed() {
    let dir = install_dir("warns");
    // What the loader prints of a symbol version the host's C library lacks
    // but the binary only weakly needs: it runs, and says its build.
    let script = b"#!/bin/sh\necho \"weak version 'GLIBC_2.39' not found\" >&2\necho abc\n";
    run(&HERE, &copy_command(&dir, "abc"), script)
        .await
        .unwrap();
    assert!(Path::new(&dir).join("onehand-host").exists());
    assert!(!Path::new(&dir).join("onehand-host.tmp").exists());
    assert!(!Path::new(&dir).join("onehand-host.tmp.err").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn a_copy_that_names_another_build_says_what_it_said() {
    let dir = install_dir("other");
    let script = b"#!/bin/sh\necho xyz\n";
    let copied = run(&HERE, &copy_command(&dir, "abc"), script).await;
    assert!(
        matches!(&copied, Err(HostError::Unreachable(why)) if why.contains("xyz")),
        "{copied:?}"
    );
    assert!(!Path::new(&dir).join("onehand-host").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn a_cut_short_copy_is_never_installed() {
    let dir = install_dir("short");
    // What a copy cut off mid-way leaves: a file that cannot name the build.
    let script = b"#!/bin/sh\necho ab";
    let copied = run(&HERE, &copy_command(&dir, "abc"), script).await;
    assert!(matches!(copied, Err(HostError::Unreachable(why)) if why.contains("cut short")));
    assert!(!Path::new(&dir).join("onehand-host").exists());
    assert!(!Path::new(&dir).join("onehand-host.tmp").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn a_copy_that_cannot_run_here_says_why_and_is_never_installed() {
    let dir = install_dir("old-glibc");
    // What a binary built against a newer C library prints as it fails to load.
    let script = b"#!/bin/sh\necho \"version 'GLIBC_2.39' not found\" >&2\nexit 1\n";
    let copied = run(&HERE, &copy_command(&dir, "abc"), script).await;
    assert!(
        matches!(&copied, Err(HostError::Unreachable(why))
            if why.contains("GLIBC_2.39") && !why.contains("cut short")),
        "{copied:?}"
    );
    assert!(!Path::new(&dir).join("onehand-host").exists());
    assert!(!Path::new(&dir).join("onehand-host.tmp").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_probe_is_read_after_its_marker_whatever_an_rc_file_prints() {
    let out = format!("Welcome to the box!\n{PROBE}\nLinux x86_64\npresent\n");
    assert_eq!(probed(&out), ("Linux x86_64", true));
    let out = format!("{PROBE} is a word too\n{PROBE}\nDarwin arm64\n");
    assert_eq!(probed(&out), ("Darwin arm64", false));
    assert_eq!(probed("banner only\n"), ("", false));
}
