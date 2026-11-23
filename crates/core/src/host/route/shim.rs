//! `onehand --route`, and the shims that call it: one small script per
//! routed program, in a folder put first on the agent's PATH, so the
//! agent's `cargo` reaches the running onehand, which builds it on the
//! project's build host or answers that it runs here.

use std::ffi::{OsStr, OsString};
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};

use super::call::{client_blocking, RouteEnd, Unreached};
use crate::host::build::shell::Shell;

/// Set for a command run here once its route was settled (builds on hosts
/// off, or the host or onehand out of reach), so a shim it starts in turn
/// runs here at once rather than asking again.
const ROUTED_HERE: &str = "ONEHAND_ROUTED_HERE";

/// The exit code a shell gives a command it could not find.
const NOT_FOUND: i32 = 127;

/// The exit code a shell gives a command it found but could not run.
const NOT_RUN: i32 = 126;

/// The folder the shims are written to.
pub(crate) fn shim_dir(config: &Path) -> PathBuf {
    config.join("route-shims")
}

/// The whole of `onehand --route <socket> <shims> <program> <args…>`: the
/// command built where onehand decides, and its exit code. Returns only when
/// the command did not run here; running here replaces this process.
///
/// `shims` is the folder the calling shim lives in, named by the shim
/// itself: the agent's environment may not lead to the config folder
/// onehand wrote it in, and the real program is looked for outside it.
pub fn route_main(socket: &Path, shims: &Path, program: &str, args: &[String]) -> i32 {
    route_in(
        socket,
        program,
        args,
        std::env::var_os(ROUTED_HERE).is_some(),
        std::env::var_os("PATH").as_deref(),
        shims,
    )
}

pub(super) fn route_in(
    socket: &Path,
    program: &str,
    args: &[String],
    routed_here: bool,
    path: Option<&OsStr>,
    shims: &Path,
) -> i32 {
    let here = |settled: bool| run_here(program, args, path, shims, settled);
    if routed_here {
        return here(true);
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let (out, err) = (std::io::stdout(), std::io::stderr());
    let end = client_blocking(
        socket,
        program,
        args,
        &cwd,
        &mut out.lock(),
        &mut err.lock(),
    );
    match end {
        Ok(RouteEnd::Exit(code)) => code,
        Ok(RouteEnd::Here { settled }) => here(settled),
        Err(Unreached::Blocked) => {
            eprintln!(
                "onehand: the agent's sandbox blocks onehand's socket; allow network in its sandbox; building here"
            );
            here(true)
        }
        Err(Unreached::Gone(_)) => {
            eprintln!("onehand: onehand is not reachable; building here");
            here(true)
        }
    }
}

/// Replace this process with the real `program`, marked when its route was
/// settled here; the code to exit with when it cannot.
fn run_here(
    program: &str,
    args: &[String],
    path: Option<&OsStr>,
    shims: &Path,
    settled: bool,
) -> i32 {
    use std::os::unix::process::CommandExt as _;
    let Some(real) = real_program(program, path, shims) else {
        eprintln!("onehand: {program}: command not found");
        return NOT_FOUND;
    };
    let mut command = std::process::Command::new(real);
    command.args(args);
    if settled {
        command.env(ROUTED_HERE, "1");
    }
    let why = command.exec();
    eprintln!("onehand: {program}: {why}");
    NOT_RUN
}

/// The first executable `program` on `path` outside the shim folder.
/// `None` too when what is found is a shim still, reached through a link:
/// running it would call itself for ever.
pub(crate) fn real_program(
    program: &str,
    path: Option<&OsStr>,
    shim_dir: &Path,
) -> Option<PathBuf> {
    let shims = shim_dir.canonicalize().ok();
    let found = std::env::split_paths(path?)
        .filter(|dir| shims.is_none() || dir.canonicalize().ok() != shims)
        .map(|dir| dir.join(program))
        .find(|file| {
            std::fs::metadata(file)
                .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
        })?;
    let target = found.canonicalize().ok();
    let in_shims = shims.is_some() && target.as_deref().and_then(Path::parent) == shims.as_deref();
    (!in_shims).then_some(found)
}

/// One executable shim in `dir` per program, handing it to `exe --route`
/// on `socket` with `dir` itself; a shim for a program no longer named is
/// removed. Only a program a routed command may name gets one.
pub(crate) fn write_shims_blocking(
    dir: &Path,
    exe: &Path,
    socket: &Path,
    programs: &[String],
) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let word = |path: &Path| Shell::Posix.word(&path.to_string_lossy());
    let (exe, socket, shims) = (
        word(exe).map_err(std::io::Error::other)?,
        word(socket).map_err(std::io::Error::other)?,
        word(dir).map_err(std::io::Error::other)?,
    );
    let programs: Vec<&String> = programs
        .iter()
        .filter(|p| crate::workspace::plain_program(p))
        .collect();
    for entry in std::fs::read_dir(dir)?.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        // Kept: a shim still named, or one on its way in.
        let kept = |p: &&String| name == **p || name.starts_with(&format!("{p}.tmp"));
        if !programs.iter().any(kept) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
    for program in programs {
        // With its onehand gone (a newer build replaced the file), the
        // shim takes itself off PATH and runs the real program.
        let text = format!(
            "#!/bin/sh\n\
             [ -x {exe} ] && exec {exe} --route {socket} {shims} {program} \"$@\"\n\
             set -f; IFS=:; p=; for d in $PATH; do [ \"$d\" = {shims} ] || p=${{p:+$p:}}$d; done; \
             unset IFS; set +f; PATH=$p; export PATH\n\
             exec {program} \"$@\"\n"
        );
        let file = dir.join(program);
        if std::fs::read_to_string(&file).is_ok_and(|had| had == text) {
            continue;
        }
        // Whole, because an agent may be running the old one. A rewrite
        // keeps the mode; only a new shim is made executable after.
        crate::config::write_atomic(&file, &text)?;
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
}

/// The PATH an agent starts with: the shim folder first, then `base`, the
/// PATH it would have had; `None` when no project routes a command, or the
/// shims could not be written. Blocking: it reads every project file.
pub fn agent_path_blocking(base: Option<&OsStr>) -> Option<OsString> {
    let config = crate::config::config_dir();
    let (projects, _) =
        crate::project::file::load_all_blocking(&crate::config::projects_in(&config));
    let mut programs: Vec<String> = projects
        .iter()
        .flat_map(|project| project.routes.iter().map(|r| r.program().to_string()))
        .collect();
    programs.sort();
    programs.dedup();
    if programs.is_empty() {
        return None;
    }
    let dir = refresh_blocking(&programs).ok()?;
    Some(agent_path(&dir, base))
}

/// Write the shim folder for `programs`, every one any project routes, a
/// shim for any other removed; the folder. Carries out the projects'
/// `Effect::Shims`, so an agent already running with the folder on its
/// PATH reaches a program routed since it started.
pub fn refresh_blocking(programs: &[String]) -> std::io::Result<PathBuf> {
    let config = crate::config::config_dir();
    let dir = shim_dir(&config);
    let exe = live_exe(std::env::current_exe()?);
    let socket = crate::instance::socket_path(&config);
    write_shims_blocking(&dir, &exe, &socket, programs)?;
    Ok(dir)
}

/// `exe` as its file is named: Linux names a running binary whose file was
/// replaced, as every cargo build replaces it, with ` (deleted)` after it.
fn live_exe(exe: PathBuf) -> PathBuf {
    match exe.to_str().and_then(|e| e.strip_suffix(" (deleted)")) {
        Some(live) => PathBuf::from(live),
        None => exe,
    }
}

/// `shims` first, then `base`.
fn agent_path(shims: &Path, base: Option<&OsStr>) -> OsString {
    let mut path = OsString::from(shims);
    if let Some(base) = base.filter(|b| !b.is_empty()) {
        path.push(":");
        path.push(base);
    }
    path
}

#[cfg(test)]
mod tests;
