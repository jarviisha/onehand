use super::*;
use serde_json::Value;

use std::os::unix::net::UnixListener;

fn socket_path(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("onehand-route-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("onehand.sock")
}

#[test]
fn route_ask_reads_only_route_lines() {
    let args = vec!["build".to_string(), "--release".to_string()];
    let line = route_request("cargo", &args, Path::new("/p/crates/a"));
    let ask = route_ask(&line).unwrap();
    assert_eq!(ask.program, "cargo");
    assert_eq!(ask.args, args);
    assert_eq!(ask.cwd, PathBuf::from("/p/crates/a"));
    assert!(route_ask(&crate::instance::open_request(None)).is_none());
}

#[test]
fn lines_and_ends_are_json_rpc() {
    let parse = |s: String| serde_json::from_str::<Value>(&s).unwrap();
    let out = parse(said_line(&Said::Out("hi".into())).unwrap());
    assert_eq!(out["jsonrpc"], "2.0");
    assert_eq!(out["method"], "onehand/line");
    assert_eq!(out["params"]["out"], "hi");
    let note = parse(said_line(&Said::Note("n".into())).unwrap());
    assert_eq!(note["params"]["note"], "n");
    let here = parse(end_line(&RouteEnd::Here { fell_back: true }));
    assert_eq!(here["id"], 1);
    assert_eq!(here["result"]["run"], "here");
    assert_eq!(here["result"]["fell_back"], true);
    assert_eq!(said_line(&Said::Reached), None);
    let exit = parse(end_line(&RouteEnd::Exit(3)));
    assert_eq!(exit["result"]["exit"], 3);
}

#[test]
fn client_prints_lines_and_returns_the_exit() {
    let path = socket_path("client");
    let listener = UnixListener::bind(&path).unwrap();
    let served = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut first = String::new();
        std::io::BufReader::new(&stream)
            .read_line(&mut first)
            .unwrap();
        for line in [
            said_line(&Said::Out("one".into())).unwrap(),
            said_line(&Said::Note("waiting".into())).unwrap(),
            said_line(&Said::Out("two".into())).unwrap(),
            end_line(&RouteEnd::Exit(3)),
        ] {
            writeln!(stream, "{line}").unwrap();
        }
        first
    });
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let end = client_blocking(&path, "cargo", &[], Path::new("/p"), &mut out, &mut err);
    assert_eq!(end, Ok(RouteEnd::Exit(3)));
    assert_eq!(String::from_utf8(out).unwrap(), "one\ntwo\n");
    assert_eq!(String::from_utf8(err).unwrap(), "onehand: waiting\n");
    assert!(route_ask(&served.join().unwrap()).is_some());
}

#[test]
fn client_reports_an_unreachable_socket() {
    let path = socket_path("none");
    let end = client_blocking(
        &path,
        "cargo",
        &[],
        Path::new("/p"),
        &mut Vec::new(),
        &mut Vec::new(),
    );
    assert!(matches!(end, Err(Unreached::Gone(_))), "{end:?}");
}

#[test]
fn a_relative_folder_runs_here() {
    let ask = Ask {
        program: "cargo".into(),
        args: vec![],
        cwd: "rel".into(),
    };
    let end = serve_blocking(
        &ask,
        &mut |_| panic!("nothing said"),
        &AtomicBool::new(false),
    );
    assert_eq!(end, RouteEnd::Here { fell_back: false });
}

#[test]
fn client_ends_on_an_error_reply() {
    let path = socket_path("error");
    let listener = UnixListener::bind(&path).unwrap();
    let served = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut first = String::new();
        std::io::BufReader::new(&stream)
            .read_line(&mut first)
            .unwrap();
        let error =
            r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"no such method"}}"#;
        writeln!(stream, "{error}").unwrap();
        // Held open: a client that waits for more would hang here.
        std::thread::sleep(std::time::Duration::from_secs(5));
    });
    let end = client_blocking(
        &path,
        "cargo",
        &[],
        Path::new("/p"),
        &mut Vec::new(),
        &mut Vec::new(),
    );
    assert_eq!(end, Err(Unreached::Gone("no such method".into())));
    drop(served);
}
