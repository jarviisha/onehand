use super::*;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("onehand-shim-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn executable(path: &Path) {
    std::fs::write(path, "#!/bin/sh\n").unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn real_program_skips_the_shim_folder() {
    let root = scratch("real");
    let (shim, a, b) = (root.join("shim"), root.join("a"), root.join("b"));
    for dir in [&shim, &a, &b] {
        std::fs::create_dir_all(dir).unwrap();
    }
    executable(&shim.join("cargo"));
    executable(&b.join("cargo"));
    let path = std::env::join_paths([&shim, &a, &b]).unwrap();
    assert_eq!(
        real_program("cargo", Some(&path), &shim),
        Some(b.join("cargo"))
    );
    assert_eq!(real_program("dotnet", Some(&path), &shim), None);
}

#[test]
fn missing_real_program_exits_127() {
    let root = scratch("missing");
    let path = std::env::join_paths([root.join("nothing")]).unwrap();
    let code = route_in(
        Path::new("/nonexistent.sock"),
        "onehand-no-such-program",
        &[],
        true,
        Some(&path),
        &root.join("shim"),
    );
    assert_eq!(code, 127);
}

#[test]
fn shims_are_written_and_pruned() {
    let dir = scratch("written").join("route-shims");
    let (exe, socket) = (Path::new("/opt/onehand"), Path::new("/run/onehand.sock"));
    let programs = |names: &[&str]| names.iter().map(|n| n.to_string()).collect::<Vec<_>>();
    write_shims_blocking(&dir, exe, socket, &programs(&["cargo", "dotnet"])).unwrap();
    write_shims_blocking(&dir, exe, socket, &programs(&["cargo"])).unwrap();
    let names: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(names, ["cargo"]);
    let cargo = dir.join("cargo");
    let mode = std::fs::metadata(&cargo).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o755);
    assert_eq!(
        std::fs::read_to_string(&cargo).unwrap(),
        format!(
            "#!/bin/sh\n\
             [ -x '/opt/onehand' ] && exec '/opt/onehand' --route '/run/onehand.sock' '{d}' cargo \"$@\"\n\
             set -f; IFS=:; p=; for d in $PATH; do [ \"$d\" = '{d}' ] || p=${{p:+$p:}}$d; done; \
             unset IFS; set +f; PATH=$p; export PATH\n\
             exec cargo \"$@\"\n",
            d = dir.display()
        )
    );
}

#[test]
fn shim_quotes_paths_with_spaces() {
    let dir = scratch("spaces").join("route-shims");
    let exe = Path::new("/tmp/a b/onehand");
    write_shims_blocking(&dir, exe, Path::new("/s.sock"), &["cargo".into()]).unwrap();
    let text = std::fs::read_to_string(dir.join("cargo")).unwrap();
    assert!(text.contains("'/tmp/a b/onehand'"), "{text}");
}

#[test]
fn shim_dir_is_in_the_config_folder() {
    assert_eq!(shim_dir(Path::new("/c")), PathBuf::from("/c/route-shims"));
}

/// The folder `--route` skips is the one its shim names, whatever the
/// config folder of the process is: a shim alone on PATH is never run as
/// the real program, which would call itself for ever.
#[test]
fn a_shim_never_runs_itself() {
    let root = scratch("itself");
    let shim = root.join("elsewhere-shims");
    std::fs::create_dir_all(&shim).unwrap();
    executable(&shim.join("cargo"));
    assert_ne!(shim, shim_dir(&crate::config::config_dir()));
    let path = std::env::join_paths([&shim]).unwrap();
    let code = route_in(
        Path::new("/nonexistent.sock"),
        "cargo",
        &[],
        true,
        Some(&path),
        &shim,
    );
    assert_eq!(code, 127);
}

/// A link elsewhere on PATH to a shim is the shim still.
#[test]
fn a_link_to_a_shim_is_not_the_real_program() {
    let root = scratch("link");
    let (shim, other) = (root.join("shim"), root.join("other"));
    for dir in [&shim, &other] {
        std::fs::create_dir_all(dir).unwrap();
    }
    executable(&shim.join("cargo"));
    std::os::unix::fs::symlink(shim.join("cargo"), other.join("cargo")).unwrap();
    let path = std::env::join_paths([&other]).unwrap();
    assert_eq!(real_program("cargo", Some(&path), &shim), None);
}

#[test]
fn an_agent_path_leads_with_the_shims() {
    let path = agent_path(Path::new("/s"), Some(OsStr::new("/a:/b")));
    assert_eq!(path, OsString::from("/s:/a:/b"));
    assert_eq!(agent_path(Path::new("/s"), None), OsString::from("/s"));
}

/// A shim whose onehand is gone, its file replaced by a newer build or
/// removed, runs the real program, never itself.
#[test]
fn a_shim_without_its_onehand_runs_the_real_program() {
    let root = scratch("orphan");
    let (shims, real) = (root.join("route-shims"), root.join("real"));
    std::fs::create_dir_all(&real).unwrap();
    let program = "onehand-shim-test-prog";
    std::fs::write(real.join(program), "#!/bin/sh\nexit \"$1\"\n").unwrap();
    std::fs::set_permissions(real.join(program), std::fs::Permissions::from_mode(0o755)).unwrap();
    let gone = root.join("no-onehand-here");
    write_shims_blocking(&shims, &gone, Path::new("/s.sock"), &[program.into()]).unwrap();
    let path = std::env::join_paths([&shims, &real, Path::new("/usr/bin"), Path::new("/bin")])
        .unwrap();
    // Bounded, as a shim that found itself again would never end.
    let status = std::process::Command::new("timeout")
        .args(["10"])
        .arg(shims.join(program))
        .arg("7")
        .env("PATH", path)
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(7));
}

#[test]
fn a_replaced_binary_is_named_as_its_file() {
    assert_eq!(
        live_exe(PathBuf::from("/t/onehand (deleted)")),
        PathBuf::from("/t/onehand")
    );
    assert_eq!(live_exe(PathBuf::from("/t/onehand")), PathBuf::from("/t/onehand"));
}
