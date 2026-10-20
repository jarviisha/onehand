//! Which repository a checkout's `origin` names, and how it is reached:
//! read locally, so a project that can never be worked costs nothing but
//! this.

use super::LOCAL_LIMIT;
use std::path::Path;
use std::process::Command;

/// The URL of `root`'s `origin`, read locally.
pub(super) fn origin_url(root: &Path) -> Result<String, String> {
    let out = onehand_core::process::output_within(
        Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["remote", "get-url", "origin"]),
        LOCAL_LIMIT,
    )
    .map_err(|err| format!("git {err}"))?;
    if !out.status.success() {
        return Err("it has no `origin` remote to open a pull request against".to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// The HTTPS URL of the repository an ssh remote `url` names, on the host ssh
/// would actually reach — an alias such as `github-work` is `resolve`d first.
/// `None` for a remote that is not over ssh, where there is nothing different
/// to try.
pub(super) fn https_url(url: &str, resolve: impl Fn(&str) -> Option<String>) -> Option<String> {
    if !over_ssh(url) {
        return None;
    }
    let host = remote_host(url)?;
    let path = match url.split_once("://") {
        // `ssh://user@host[:port]/path`: everything after the host.
        Some((_, rest)) => rest.split_once('/')?.1,
        // `user@host:path`.
        None => url.split_once(':')?.1,
    };
    let host = resolve(host).unwrap_or_else(|| host.to_string());
    Some(format!("https://{host}/{}", path.trim_start_matches('/')))
}

/// The host a git remote URL points at: `https://host/…`, `ssh://user@host:port/…`
/// and the scp-like `user@host:path`. `None` for a local path.
pub(super) fn remote_host(url: &str) -> Option<&str> {
    let rest = match url.split_once("://") {
        Some((_, rest)) => rest,
        // scp-like: a colon before any slash, and the part before it is a host.
        None => match url.split_once(':') {
            Some((host, _)) if !host.contains('/') => host,
            _ => return None,
        },
    };
    let host = rest.split('/').next()?;
    let host = host.rsplit('@').next()?;
    let host = host.split(':').next()?;
    (!host.is_empty()).then_some(host)
}

/// Whether `url` is a remote a run can work: one on github.com.
///
/// **An ssh remote is judged by the host ssh would actually reach**, which
/// `resolve` answers. The host in an ssh URL can be an alias from the user's ssh
/// configuration — `git@github-work:me/repo`, which is how one machine keeps
/// two GitHub accounts apart — and reading the word in the URL refused every
/// such project as not being on GitHub. An https remote has no alias and is
/// read as written.
pub(super) fn github_remote(
    url: &str,
    resolve: impl Fn(&str) -> Option<String>,
) -> Result<(), String> {
    let refuse = |host: &str| Err(format!("its remote is on {host}, not GitHub"));
    let Some(host) = remote_host(url) else {
        return Err("its remote is not on GitHub".to_string());
    };
    if host == "github.com" {
        return Ok(());
    }
    if !over_ssh(url) {
        return refuse(host);
    }
    match resolve(host) {
        Some(real) if real == "github.com" => Ok(()),
        Some(real) => refuse(&real),
        None => refuse(host),
    }
}

/// Whether `url` reaches its host over ssh: the scp-like form, or an `ssh`
/// scheme.
pub(super) fn over_ssh(url: &str) -> bool {
    match url.split_once("://") {
        Some((scheme, _)) => scheme.contains("ssh"),
        None => true,
    }
}

/// The `hostname` line of what `ssh -G` printed: the host an alias stands for,
/// after the user's ssh configuration has been applied.
pub(super) fn ssh_hostname(said: &str) -> Option<&str> {
    said.lines()
        .find_map(|line| line.strip_prefix("hostname "))
        .map(str::trim)
}

/// Ask ssh which host `alias` stands for. `ssh -G` only prints the settled
/// configuration; it connects to nothing.
pub(super) fn ssh_resolve_blocking(alias: &str) -> Option<String> {
    let out =
        onehand_core::process::output_within(Command::new("ssh").arg("-G").arg(alias), LOCAL_LIMIT)
            .ok()?;
    out.status.success().then_some(())?;
    ssh_hostname(&String::from_utf8_lossy(&out.stdout)).map(str::to_string)
}
