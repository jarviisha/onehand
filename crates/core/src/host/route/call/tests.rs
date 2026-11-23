use super::*;
use serde_json::Value;

use super::super::drive::RouteDrive;
use crate::workspace::HostName;

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
    let out = parse(said_lines(&Said::Out("hi".into())).remove(0));
    assert_eq!(out["jsonrpc"], "2.0");
    assert_eq!(out["method"], "onehand/line");
    assert_eq!(out["params"]["out"], "hi");
    let note = parse(said_lines(&Said::Note("n".into())).remove(0));
    assert_eq!(note["params"]["note"], "n");
    let here = parse(end_line(&RouteEnd::Here { settled: true }));
    assert_eq!(here["id"], 1);
    assert_eq!(here["result"]["run"], "here");
    assert_eq!(here["result"]["settled"], true);
    assert!(said_lines(&Said::Reached).is_empty());
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
            said_lines(&Said::Out("one".into())).remove(0),
            said_lines(&Said::Note("waiting".into())).remove(0),
            said_lines(&Said::Out("two".into())).remove(0),
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
    assert_eq!(end, RouteEnd::Here { settled: false });
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

#[test]
fn client_reads_a_settled_end() {
    let path = socket_path("settled");
    let listener = UnixListener::bind(&path).unwrap();
    let served = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut first = String::new();
        std::io::BufReader::new(&stream)
            .read_line(&mut first)
            .unwrap();
        writeln!(stream, "{}", end_line(&RouteEnd::Here { settled: true })).unwrap();
    });
    let end = client_blocking(
        &path,
        "cargo",
        &[],
        Path::new("/p"),
        &mut Vec::new(),
        &mut Vec::new(),
    );
    assert_eq!(end, Ok(RouteEnd::Here { settled: true }));
    served.join().unwrap();
}

fn drive_in(name: &str) -> std::sync::Mutex<RouteDrive> {
    let dir = socket_path(name);
    std::sync::Mutex::new(RouteDrive::load(dir.parent().unwrap()))
}

/// Where the run goes is said once the host answered or did not, never
/// before: a fall-back says only that it builds here.
#[test]
fn where_a_run_goes_is_said_after_the_host_answers() {
    let host = HostName("h".into());
    // One that answers says nothing yet: the build says when it runs.
    for (answers, said) in [
        (true, None),
        (false, Some("h did not answer; building here")),
    ] {
        let drive = drive_in(&format!("take-{answers}"));
        let mut notes = Vec::new();
        let took = take(
            &host,
            || drive.lock().unwrap(),
            |_| answers,
            &mut |s| match s {
                Said::Note(n) => notes.push(n),
                Said::Started(h) => notes.push(format!("running on {h}")),
                Said::Out(_) | Said::Reached => {}
            },
        );
        assert_eq!(took, answers);
        assert_eq!(notes, Vec::from_iter(said));
    }
}

/// A host that did not answer moments ago is not asked again.
#[test]
fn a_host_just_down_is_not_asked() {
    let host = HostName("h".into());
    let drive = drive_in("take-down");
    drive.lock().unwrap().reached(&host, false);
    let mut notes = Vec::new();
    let took = take(
        &host,
        || drive.lock().unwrap(),
        |_| panic!("not asked"),
        &mut |s| match s {
            Said::Note(n) => notes.push(n),
            Said::Started(h) => notes.push(format!("running on {h}")),
            Said::Out(_) | Said::Reached => {}
        },
    );
    assert!(!took);
    assert_eq!(notes, ["h did not answer; building here"]);
}

/// A command's line too long for the socket goes as several, each under
/// the line cap, and reads back as the whole.
#[test]
fn a_long_out_line_is_split() {
    for long in ["é".repeat(40 * 1024), "\x1b[31m\"\\".repeat(30 * 1024)] {
        split_under_the_cap(long);
    }
}

fn split_under_the_cap(long: String) {
    let lines = said_lines(&Said::Out(long.clone()));
    assert!(lines.len() > 1);
    let mut back = String::new();
    for line in &lines {
        assert!(line.len() < LINE_CAP, "{}", line.len());
        let msg: Value = serde_json::from_str(line).unwrap();
        back.push_str(msg["params"]["out"].as_str().unwrap());
    }
    assert_eq!(back, long);
}

/// A reply line past the cap is refused, not held in memory.
#[test]
fn client_refuses_a_line_past_the_cap() {
    let path = socket_path("cap");
    let listener = UnixListener::bind(&path).unwrap();
    let served = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut first = String::new();
        std::io::BufReader::new(&stream)
            .read_line(&mut first)
            .unwrap();
        let _ = writeln!(stream, "{}", "x".repeat(LINE_CAP + 10));
    });
    let end = client_blocking(
        &path,
        "cargo",
        &[],
        Path::new("/p"),
        &mut Vec::new(),
        &mut Vec::new(),
    );
    assert!(matches!(end, Err(Unreached::Gone(_))), "{end:?}");
    served.join().unwrap();
}

/// A client served `lines` and then dropped, or sent a line past the cap
/// when `long`: how it ends, and what it said on stderr.
fn cut_after(name: &str, lines: Vec<String>, long: bool) -> (Result<RouteEnd, Unreached>, String) {
    let path = socket_path(name);
    let listener = UnixListener::bind(&path).unwrap();
    let served = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut first = String::new();
        std::io::BufReader::new(&stream)
            .read_line(&mut first)
            .unwrap();
        for line in lines {
            writeln!(stream, "{line}").unwrap();
        }
        if long {
            let _ = writeln!(stream, "{}", "x".repeat(LINE_CAP + 10));
        }
    });
    let mut err = Vec::new();
    let end = client_blocking(
        &path,
        "cargo",
        &[],
        Path::new("/p"),
        &mut Vec::new(),
        &mut err,
    );
    served.join().unwrap();
    (end, String::from_utf8(err).unwrap())
}

/// Once the command started on the host, a lost connection or an
/// overlong line is the build failing, never a cue to run it here again.
#[test]
fn a_run_cut_after_it_started_is_not_run_again() {
    let started = said_lines(&Said::Started(HostName("h".into()))).remove(0);
    let (end, err) = cut_after("cut-started", vec![started.clone()], false);
    assert_eq!(
        end,
        Err(Unreached::Cut(
            "onehand went away during the build on h".into()
        ))
    );
    assert_eq!(err, "onehand: running on h\n");
    let (end, _) = cut_after("cut-long", vec![started], true);
    assert_eq!(end, Err(Unreached::Cut("a reply line was too long".into())));
    let out = said_lines(&Said::Out("one".into())).remove(0);
    let (end, _) = cut_after("cut-out", vec![out], false);
    assert!(matches!(end, Err(Unreached::Cut(_))), "{end:?}");
}

/// Cut before anything started, the run falls back as ever.
#[test]
fn a_run_cut_before_it_started_falls_back() {
    let waiting = said_lines(&Said::Note("waiting".into())).remove(0);
    let (end, _) = cut_after("cut-before", vec![waiting], false);
    assert!(matches!(end, Err(Unreached::Gone(_))), "{end:?}");
    let (end, _) = cut_after("cut-before-long", Vec::new(), true);
    assert!(matches!(end, Err(Unreached::Gone(_))), "{end:?}");
}
