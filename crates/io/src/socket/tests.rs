use super::*;
use std::path::PathBuf;
use std::sync::atomic::Ordering;

#[test]
fn dropped_connection_cancels_the_build() {
    let dir = std::env::temp_dir().join(format!("onehand-io-route-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("onehand.sock");
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap();
    let listener = {
        let _in = rt.enter();
        UnixListener::bind(&path).unwrap()
    };
    let (seen_tx, seen) = std::sync::mpsc::channel();
    let (tx, _rx) = unbounded();
    rt.spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let (read, write) = stream.into_split();
        let ask = Ask {
            program: "cargo".into(),
            args: vec![],
            cwd: PathBuf::from("/p"),
        };
        let serve = move |_: &Ask, _: &mut dyn FnMut(Said), cancel: &AtomicBool| {
            let started = std::time::Instant::now();
            while !cancel.load(Ordering::Relaxed) && started.elapsed() < Duration::from_secs(5) {
                std::thread::sleep(Duration::from_millis(10));
            }
            let _ = seen_tx.send(cancel.load(Ordering::Relaxed));
            RouteEnd::Exit(0)
        };
        serve_route(ask, BufReader::new(read), write, tx, serve).await;
    });
    let client = std::os::unix::net::UnixStream::connect(&path).unwrap();
    std::thread::sleep(Duration::from_millis(100));
    drop(client);
    assert_eq!(seen.recv_timeout(Duration::from_secs(2)), Ok(true));
}

/// A host's standing changing mid-run reaches the front end then, not only
/// when the run ends.
#[test]
fn a_standing_changed_reaches_the_front_end_during_the_run() {
    let dir = std::env::temp_dir().join(format!("onehand-io-reached-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("onehand.sock");
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap();
    let listener = {
        let _in = rt.enter();
        UnixListener::bind(&path).unwrap()
    };
    let (tx, mut rx) = unbounded();
    let (done_tx, done) = std::sync::mpsc::channel::<()>();
    rt.spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let (read, write) = stream.into_split();
        let ask = Ask {
            program: "cargo".into(),
            args: vec![],
            cwd: PathBuf::from("/p"),
        };
        let serve = move |_: &Ask, emit: &mut dyn FnMut(Said), _: &AtomicBool| {
            emit(Said::Reached);
            // Held until the test has seen it.
            let _ = done.recv_timeout(Duration::from_secs(5));
            RouteEnd::Exit(0)
        };
        serve_route(ask, BufReader::new(read), write, tx, serve).await;
    });
    let _client = std::os::unix::net::UnixStream::connect(&path).unwrap();
    let started = std::time::Instant::now();
    let seen = loop {
        match rx.try_recv() {
            Ok(Incoming::Routed) => break true,
            Ok(Incoming::Open(_) | Incoming::Call(..)) | Err(_) => {}
        }
        if started.elapsed() > Duration::from_secs(2) {
            break false;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let _ = done_tx.send(());
    assert!(seen, "the front end heard nothing while the run went on");
}
