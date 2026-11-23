//! `onehand --route`, and the shims that call it: one small script per
//! routed program, in a folder put first on the agent's PATH, so the
//! agent's `cargo` reaches the running onehand, which builds it on the
//! project's build host or answers that it runs here.

use std::ffi::{OsStr, OsString};
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};

use super::call::{client_blocking, RouteEnd, Unreached};
use crate::host::build::shell::Shell;

/// Set for a command run here because its route fell through, so a shim it
/// starts in turn runs here at once rather than asking again.
const ROUTED_HERE: &str = "ONEHAND_ROUTED_HERE";

/// The exit code a shell gives a command it could not find.
const NOT_FOUND: i32 = 127;

/// The folder the shims are written to.
pub fn shim_dir(config: &Path) -> PathBuf {
    config.join("route-shims")
}

/// The whole of `onehand --route <socket> <program> <args…>`: the command
/// built where onehand decides, and its exit code. Returns only when the
/// command did not run here; running here replaces this process.
pub fn route_main(socket: &Path, program: &str, args: &[String]) -> i32 {
    let config = crate::config::config_dir();
    route_in(
        socket,
        program,
        args,
        std::env::var_os(ROUTED_HERE).is_some(),
        std::env::var_os("PATH").as_deref(),
        &shim_dir(&config),
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
    let here = |fell_back: bool| run_here(program, args, path, shims, fell_back);
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
        Ok(RouteEnd::Here { fell_back }) => here(fell_back),
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

/// Replace this process with the real `program`, marked when it runs here
/// because its route fell through; the code to exit with when it cannot.
fn run_here(
    program: &str,
    args: &[String],
    path: Option<&OsStr>,
    shims: &Path,
    fell_back: bool,
) -> i32 {
    use std::os::unix::process::CommandExt as _;
    let Some(real) = real_program(program, path, shims) else {
        eprintln!("onehand: {program}: command not found");
        return NOT_FOUND;
    };
    let mut command = std::process::Command::new(real);
    command.args(args);
    if fell_back {
        command.env(ROUTED_HERE, "1");
    }
    let why = command.exec();
    eprintln!("onehand: {program}: {why}");
    126
}

/// The first executable `program` on `path` outside the shim folder.
pub(crate) fn real_program(
    program: &str,
    path: Option<&OsStr>,
    shim_dir: &Path,
) -> Option<PathBuf> {
    let shims = shim_dir.canonicalize().ok();
    std::env::split_paths(path?)
        .filter(|dir| shims.is_none() || dir.canonicalize().ok() != shims)
        .map(|dir| dir.join(program))
        .find(|file| {
            std::fs::metadata(file)
                .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
        })
}

/// A program a shim can stand for: a bare name, nothing a shell would read
/// as more than one word.
fn shimmable(program: &str) -> bool {
    !program.is_empty()
        && !program.starts_with('.')
        && program
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_.+".contains(c))
}

/// One executable shim in `dir` per program, handing it to `exe --route`
/// on `socket`; a shim for a program no longer named is removed.
pub fn write_shims_blocking(
    dir: &Path,
    exe: &Path,
    socket: &Path,
    programs: &[String],
) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let word = |path: &Path| Shell::Posix.word(&path.to_string_lossy());
    let (exe, socket) = (
        word(exe).map_err(std::io::Error::other)?,
        word(socket).map_err(std::io::Error::other)?,
    );
    let programs: Vec<&String> = programs.iter().filter(|p| shimmable(p)).collect();
    for entry in std::fs::read_dir(dir)?.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        // Kept: a shim still named, or one on its way in.
        let kept = |p: &&String| name == **p || name.starts_with(&format!("{p}.tmp"));
        if !programs.iter().any(kept) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
    for program in programs {
        let text = format!("#!/bin/sh\nexec {exe} --route {socket} {program} \"$@\"\n");
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

/// The PATH an agent starts with: the shim folder first, then the one this
/// process inherited; `None` when no project routes a command, or the shims
/// could not be written. Blocking: it reads every project file.
pub fn agent_path_blocking() -> Option<OsString> {
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
    let dir = shim_dir(&config);
    let exe = std::env::current_exe().ok()?;
    let socket = crate::instance::socket_path(&config);
    write_shims_blocking(&dir, &exe, &socket, &programs).ok()?;
    let inherited = std::env::var_os("PATH").unwrap_or_default();
    let mut path = OsString::from(dir);
    path.push(":");
    path.push(inherited);
    Some(path)
}

#[cfg(test)]
mod tests;
