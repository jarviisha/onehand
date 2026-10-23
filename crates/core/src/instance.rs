//! One onehand per config directory. The first process to start takes a lock
//! on `<dir>/instance.lock` and keeps it until it exits; the kernel lets go
//! of it if the process dies. Whatever must only ever run once at a time —
//! moving the config directory's files at start — runs behind it.
//!
//! The holder also listens on a Unix socket beside the lock, and a second
//! launch hands it the folder it was given instead of starting at all. What
//! is said over it is decided here, one JSON-RPC line each way, so both ends
//! read the same rule: the app only carries the lines.

use serde_json::{json, Value};
use std::fs::{File, TryLockError};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

/// Another process holds the lock on this config directory.
#[derive(Debug)]
pub struct Held;

/// The lock, kept for as long as the process lives.
static LOCK: OnceLock<File> = OnceLock::new();

/// Take the lock on `dir` for the rest of this process, or say another
/// process has it. A lock that cannot be taken at all (a read-only
/// directory, say) is reported and let go: it must not keep the app from
/// starting.
pub fn hold_lock(dir: &Path) -> Result<(), Held> {
    match try_lock(dir) {
        Ok(file) => {
            let _ = LOCK.set(file);
            Ok(())
        }
        Err(TryLockError::WouldBlock) => Err(Held),
        Err(TryLockError::Error(why)) => {
            eprintln!("onehand: no lock on {}: {why}", dir.display());
            Ok(())
        }
    }
}

/// Whether this process holds the lock. Only the holder may clear a stale
/// socket and bind its own: a process that started without the lock (it could
/// not be taken at all) would otherwise pull the socket from under another.
pub fn holding() -> bool {
    LOCK.get().is_some()
}

/// The longest path a Unix socket address holds on Linux, its NUL aside.
const MAX_SOCKET_PATH: usize = 107;

/// Where the lock holder listens: beside the lock, unless that path is too
/// long for a socket address, in which case a name in the runtime directory
/// derived from `dir`, which every process on `dir` derives alike.
pub fn socket_path(dir: &Path) -> PathBuf {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").filter(|d| !d.is_empty());
    socket_path_in(dir, runtime.as_deref().map(Path::new))
}

fn socket_path_in(dir: &Path, runtime: Option<&Path>) -> PathBuf {
    use std::os::unix::ffi::OsStrExt as _;
    let beside = dir.join("onehand.sock");
    if beside.as_os_str().len() <= MAX_SOCKET_PATH {
        return beside;
    }
    // FNV-1a, written out because std's hasher may change between releases,
    // and the second launch can be a different build from the first.
    let hash = dir
        .as_os_str()
        .as_bytes()
        .iter()
        .fold(0xcbf29ce484222325u64, |h, b| {
            (h ^ u64::from(*b)).wrapping_mul(0x100000001b3)
        });
    let runtime = runtime
        .map(Path::to_path_buf)
        .unwrap_or_else(std::env::temp_dir);
    runtime.join(format!("onehand-{hash:016x}.sock"))
}

/// The line a second launch sends: open `path`, or only raise a window. The
/// path goes absolute, because the process reading it has a working
/// directory of its own.
pub fn open_request(path: Option<&Path>) -> String {
    let mut params = json!({});
    if let Some(path) = path {
        let path = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
        params["path"] = json!(path.to_string_lossy());
    }
    json!({"jsonrpc": "2.0", "id": 1, "method": "onehand/open", "params": params}).to_string()
}

/// What a second launch asked the running process to do.
#[derive(Debug, PartialEq)]
pub struct Forwarded {
    /// The folder to open as a project. `None`: only raise a window.
    pub path: Option<PathBuf>,
}

/// The reply to one line read off the socket, and what it asks for when it
/// is an `onehand/open` that can be done. Everything else is answered with a
/// JSON-RPC error rather than dropped, so the other end hears why.
pub fn answer(line: &str) -> (String, Option<Forwarded>) {
    let Ok(msg) = serde_json::from_str::<Value>(line) else {
        return (error(&Value::Null, -32700, "not a JSON-RPC line"), None);
    };
    let id = msg.get("id").cloned().unwrap_or(Value::Null);
    if msg["method"] != "onehand/open" {
        return (error(&id, -32601, "no such method"), None);
    }
    let path = msg["params"]["path"].as_str().map(PathBuf::from);
    if let Some(path) = path.as_ref().filter(|p| !p.is_dir()) {
        let why = format!("{} is not a folder", path.display());
        return (error(&id, -32602, &why), None);
    }
    let reply = json!({"jsonrpc": "2.0", "id": id, "result": {}}).to_string();
    (reply, Some(Forwarded { path }))
}

fn error(id: &Value, code: i64, message: &str) -> String {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}}).to_string()
}

/// Why a second launch could not hand its folder over.
#[derive(Debug)]
pub enum Unforwarded {
    /// Nobody answered on the socket: a build from before it, or one that
    /// could not bind it.
    Unreachable,
    /// The running process answered no, and said why.
    Refused(String),
}

/// How long a second launch waits for its answer. The running process
/// answers from its listener thread, not from a busy window.
const FORWARD_WAIT: Duration = Duration::from_secs(5);

/// Hand `path` to the process listening on `socket`, and wait for its answer.
/// Blocking: a second launch runs it before anything else is started.
pub fn forward(socket: &Path, path: Option<&Path>) -> Result<(), Unforwarded> {
    use std::io::{BufRead as _, Write as _};
    let mut stream =
        std::os::unix::net::UnixStream::connect(socket).map_err(|_| Unforwarded::Unreachable)?;
    stream
        .set_read_timeout(Some(FORWARD_WAIT))
        .map_err(|_| Unforwarded::Unreachable)?;
    writeln!(stream, "{}", open_request(path)).map_err(|_| Unforwarded::Unreachable)?;
    let mut line = String::new();
    std::io::BufReader::new(&stream)
        .read_line(&mut line)
        .map_err(|_| Unforwarded::Unreachable)?;
    let reply: Value = serde_json::from_str(&line).map_err(|_| Unforwarded::Unreachable)?;
    match reply.get("error") {
        None => Ok(()),
        Some(e) => Err(Unforwarded::Refused(
            e["message"].as_str().unwrap_or("refused").to_string(),
        )),
    }
}

fn try_lock(dir: &Path) -> Result<File, TryLockError> {
    std::fs::create_dir_all(dir).map_err(TryLockError::Error)?;
    let file = File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join("instance.lock"))
        .map_err(TryLockError::Error)?;
    file.try_lock()?;
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_lock_on_the_same_directory_is_refused_until_the_first_goes() {
        let dir = std::env::temp_dir().join(format!("onehand-instance-{}", std::process::id()));
        let first = try_lock(&dir).unwrap();
        assert!(matches!(try_lock(&dir), Err(TryLockError::WouldBlock)));
        drop(first);
        // A lock goes with the open file, and a child another test forks
        // meanwhile holds a copy of it until it execs: let go is let go
        // within a moment, not at the instant of the drop.
        let until = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while try_lock(&dir).is_err() && std::time::Instant::now() < until {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(try_lock(&dir).is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("onehand-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn reply(line: &str) -> serde_json::Value {
        serde_json::from_str(line).unwrap()
    }

    #[test]
    fn socket_path_sits_beside_the_lock() {
        let path = socket_path_in(
            Path::new("/home/u/.config/onehand"),
            Some(Path::new("/run/user/1")),
        );
        assert_eq!(path, Path::new("/home/u/.config/onehand/onehand.sock"));
    }

    #[test]
    fn a_socket_path_too_long_falls_back_to_the_runtime_dir() {
        let dir = PathBuf::from(format!("/{}", "d".repeat(119)));
        let path = socket_path_in(&dir, Some(Path::new("/run/user/1")));
        assert_eq!(path.parent(), Some(Path::new("/run/user/1")));
        let name = path.file_name().unwrap().to_str().unwrap();
        assert!(
            name.starts_with("onehand-") && name.ends_with(".sock"),
            "{name}"
        );
        assert_eq!(path, socket_path_in(&dir, Some(Path::new("/run/user/1"))));
        assert_ne!(
            path,
            socket_path_in(&dir.join("x"), Some(Path::new("/run/user/1")))
        );
    }

    #[test]
    fn open_request_carries_the_path_absolute() {
        let line = reply(&open_request(Some(Path::new("rel"))));
        let want = std::env::current_dir().unwrap().join("rel");
        assert_eq!(line["method"], "onehand/open");
        assert_eq!(line["params"]["path"], want.to_str().unwrap());
        let line = reply(&open_request(None));
        assert!(line["params"].get("path").is_none());
    }

    #[test]
    fn a_folder_is_forwarded() {
        let dir = scratch("forwarded");
        let (line, forwarded) = answer(&open_request(Some(&dir)));
        assert_eq!(forwarded.map(|f| f.path), Some(Some(dir.clone())));
        let line = reply(&line);
        assert_eq!(line["id"], 1);
        assert!(line.get("result").is_some());
        let (_, forwarded) = answer(&open_request(None));
        assert_eq!(forwarded.map(|f| f.path), Some(None));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_path_that_is_not_a_folder_is_refused() {
        let dir = scratch("not-a-folder");
        let file = dir.join("file");
        std::fs::write(&file, "").unwrap();
        let (line, forwarded) = answer(&open_request(Some(&file)));
        assert!(forwarded.is_none());
        let line = reply(&line);
        assert_eq!(line["error"]["code"], -32602);
        assert!(line["error"]["message"]
            .as_str()
            .unwrap()
            .contains("is not a folder"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_line_that_is_not_open_is_answered_with_an_error() {
        let (line, forwarded) = answer("nonsense");
        assert!(forwarded.is_none());
        assert_eq!(reply(&line)["error"]["code"], -32700);
        let (line, forwarded) = answer(r#"{"jsonrpc":"2.0","id":7,"method":"tools/list"}"#);
        assert!(forwarded.is_none());
        let line = reply(&line);
        assert_eq!(line["error"]["code"], -32601);
        assert_eq!(line["id"], 7);
    }

    #[test]
    fn forward_reaches_a_listener_and_hears_its_refusal() {
        use std::io::{BufRead as _, Write as _};
        let dir = scratch("forward");
        let socket = dir.join("onehand.sock");
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let mut stream = stream.unwrap();
                let mut line = String::new();
                std::io::BufReader::new(&stream)
                    .read_line(&mut line)
                    .unwrap();
                let (reply, _) = answer(&line);
                writeln!(stream, "{reply}").unwrap();
            }
        });
        assert!(forward(&socket, Some(&dir)).is_ok());
        let file = dir.join("file");
        std::fs::write(&file, "").unwrap();
        match forward(&socket, Some(&file)) {
            Err(Unforwarded::Refused(why)) => assert!(why.contains("is not a folder"), "{why}"),
            other => panic!("{other:?}"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn forward_to_no_listener_is_unreachable() {
        let dir = scratch("stale");
        let socket = dir.join("onehand.sock");
        // What a crashed process leaves behind: the file, and nobody on it.
        drop(std::os::unix::net::UnixListener::bind(&socket).unwrap());
        assert!(matches!(
            forward(&socket, None),
            Err(Unforwarded::Unreachable)
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
