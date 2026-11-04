use super::*;

#[test]
fn path_is_read_after_the_last_marker() {
    let out =
        format!("Welcome to the box!\n{MARKER} was here\n\n{MARKER}/usr/bin:/home/u/.nvm/bin");
    assert_eq!(
        after_marker(out.as_bytes()),
        Some(&b"/usr/bin:/home/u/.nvm/bin"[..])
    );
}

#[test]
fn no_marker_or_empty_path_is_none() {
    assert_eq!(after_marker(b"motd only"), None);
    assert_eq!(after_marker(format!("\n{MARKER}").as_bytes()), None);
}

/// One attach's end of the socket: it writes asks and reads lines.
struct Laptop {
    lines: tokio::io::Lines<BufReader<tokio::net::unix::OwnedReadHalf>>,
    write: OwnedWriteHalf,
}

impl Laptop {
    async fn new(socket: &Path) -> Laptop {
        let (read, write) = UnixStream::connect(socket).await.unwrap().into_split();
        Laptop {
            lines: BufReader::new(read).lines(),
            write,
        }
    }

    async fn ask(&mut self, id: u64, call: Call) {
        let mut text = serde_json::to_string(&Question { id, call }).unwrap();
        text.push('\n');
        self.write.write_all(text.as_bytes()).await.unwrap();
    }

    /// The next line, or `None` once the service closed this stream.
    async fn line(&mut self) -> Option<FromHost> {
        let next = tokio::time::timeout(Duration::from_secs(5), self.lines.next_line());
        let text = next.await.expect("a line in time").unwrap()?;
        Some(serde_json::from_str(&text).unwrap())
    }
}

fn terms_in() -> Arc<Mutex<Terms>> {
    Arc::new(Mutex::new(Terms::new("/bin/sh".into(), None)))
}

/// A hub whose agents come up at once, say one chunk, and run on.
fn hub_in(dir: &Path) -> Arc<Mutex<Hub>> {
    let connector: Connector = Arc::new(|_launch, _opts| {
        let (tx, _rx) = mpsc::unbounded_channel();
        futures::stream::iter([
            acp::AcpEvent::Connected {
                tx,
                resumed: false,
                forkable: false,
            },
            acp::AcpEvent::AgentChunk {
                text: "hi".into(),
                message: None,
            },
        ])
        .chain(futures::stream::pending())
        .boxed()
    });
    Arc::new(Mutex::new(Hub::new(dir.to_path_buf(), None, connector)))
}

#[tokio::test]
async fn a_watch_is_answered_after_its_replay_and_a_second_attach_kicks_the_first() {
    let dir = std::env::temp_dir().join(format!("onehand-serve-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let socket = dir.join("s.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let hub = hub_in(&dir);
    let service = tokio::spawn(accept(listener, hub, terms_in(), Build("b1".into())));

    let mut first = Laptop::new(&socket).await;
    first
        .ask(
            1,
            Call::Open {
                cwd: "/tmp".into(),
                agent: onehand_core::config::AgentSpec {
                    name: "Fake".into(),
                    command: "fake-agent".into(),
                    ..Default::default()
                },
                opening: acp::Opening::Fresh,
            },
        )
        .await;
    let Some(FromHost::Answer {
        id: 1,
        result: Ok(Reply::Opened { session }),
    }) = first.line().await
    else {
        panic!("not opened");
    };
    let journal = dir.join("sessions").join(&session.0);
    for _ in 0..500 {
        if onehand_core::host::journal::read(&journal).unwrap().len() == 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    first.ask(2, Call::Watch { session }).await;
    assert!(matches!(first.line().await, Some(FromHost::Replay { .. })));
    assert!(matches!(
        first.line().await,
        Some(FromHost::Entry { seq: 0, .. })
    ));
    assert!(matches!(
        first.line().await,
        Some(FromHost::Entry { seq: 1, .. })
    ));
    assert!(matches!(
        first.line().await,
        Some(FromHost::Answer {
            id: 2,
            result: Ok(Reply::Done)
        })
    ));

    let mut second = Laptop::new(&socket).await;
    assert!(matches!(first.line().await, Some(FromHost::Kicked)));
    assert!(first.line().await.is_none());
    second
        .ask(
            1,
            Call::Hello {
                build: Build("b1".into()),
            },
        )
        .await;
    assert!(matches!(
        second.line().await,
        Some(FromHost::Answer { id: 1, result: Ok(Reply::Welcome { build, busy }) })
            if build.0 == "b1" && busy.is_empty()
    ));
    service.abort();
    let _ = fs::remove_dir_all(&dir);
}

/// `Shutdown` itself ends the process, so only its answer is tested here.
#[tokio::test]
async fn shutdown_answers_done_and_asks_for_the_exit() {
    let dir = std::env::temp_dir().join(format!("onehand-shutdown-{}", std::process::id()));
    let (out, mut lines) = mpsc::unbounded_channel();
    let ask = serde_json::to_string(&Question {
        id: 7,
        call: Call::Shutdown,
    })
    .unwrap();
    assert!(answer(
        &hub_in(&dir),
        &terms_in(),
        &Build("b1".into()),
        &out,
        &ask
    ));
    assert!(matches!(
        lines.try_recv(),
        Ok(FromHost::Answer {
            id: 7,
            result: Ok(Reply::Done)
        })
    ));
}

#[tokio::test]
async fn probe_answers_whether_a_folder_is_there() {
    let dir = std::env::temp_dir().join(format!("onehand-probe-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let (out, mut lines) = mpsc::unbounded_channel();
    for (path, want) in [(dir.clone(), true), (dir.join("missing"), false)] {
        let ask = serde_json::to_string(&Question {
            id: 3,
            call: Call::Probe { path },
        })
        .unwrap();
        assert!(!answer(
            &hub_in(&dir),
            &terms_in(),
            &Build("b1".into()),
            &out,
            &ask
        ));
        assert!(matches!(
            lines.try_recv(),
            Ok(FromHost::Answer {
                id: 3,
                result: Ok(Reply::Probed { is_dir })
            }) if is_dir == want
        ));
    }
    let _ = fs::remove_dir_all(&dir);
}

/// Ask `call` through `answer` and wait for the answer to land on the channel.
async fn ask_file(
    dir: &Path,
    out: &UnboundedSender<FromHost>,
    lines: &mut UnboundedReceiver<FromHost>,
    id: u64,
    call: Call,
) -> Result<Reply, String> {
    let ask = serde_json::to_string(&Question { id, call }).unwrap();
    assert!(!answer(
        &hub_in(dir),
        &terms_in(),
        &Build("b1".into()),
        out,
        &ask
    ));
    match tokio::time::timeout(Duration::from_secs(5), lines.recv())
        .await
        .expect("an answer in time")
    {
        Some(FromHost::Answer { id: got, result }) if got == id => result,
        other => panic!("not the answer: {other:?}"),
    }
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("onehand-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[tokio::test]
async fn the_host_lists_a_folder() {
    let dir = scratch("list");
    fs::write(dir.join("b.txt"), "b").unwrap();
    fs::create_dir(dir.join("a")).unwrap();
    let (out, mut lines) = mpsc::unbounded_channel();
    let reply = ask_file(
        &dir,
        &out,
        &mut lines,
        1,
        Call::ReadDir { path: dir.clone() },
    )
    .await;
    let Ok(Reply::Listing { entries, cut: 0 }) = reply else {
        panic!("not a listing: {reply:?}");
    };
    let names: Vec<_> = entries.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["a", "b.txt"]);
    let _ = fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn the_host_reads_and_saves_a_file() {
    let dir = scratch("save");
    let file = dir.join("f.txt");
    fs::write(&file, "one").unwrap();
    let (out, mut lines) = mpsc::unbounded_channel();
    let Ok(Reply::File { text, mtime }) = ask_file(
        &dir,
        &out,
        &mut lines,
        1,
        Call::ReadFile { path: file.clone() },
    )
    .await
    else {
        panic!("not read");
    };
    assert_eq!(text, "one");
    let save = |text: &str, expected| Call::SaveFile {
        path: file.clone(),
        text: text.into(),
        expected,
    };
    let saved = ask_file(&dir, &out, &mut lines, 2, save("two", mtime)).await;
    assert!(matches!(
        saved,
        Ok(Reply::Saved(
            onehand_core::editor::SaveOutcome::Saved { .. }
        ))
    ));
    fs::write(&file, "three").unwrap();
    tokio::time::sleep(Duration::from_millis(10)).await;
    fs::write(&file, "four").unwrap();
    let stale = ask_file(&dir, &out, &mut lines, 3, save("five", mtime)).await;
    assert!(matches!(
        stale,
        Ok(Reply::Saved(
            onehand_core::editor::SaveOutcome::Conflict { .. }
        ))
    ));
    let _ = fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn a_missing_file_is_refused() {
    let dir = scratch("missing");
    let (out, mut lines) = mpsc::unbounded_channel();
    let reply = ask_file(
        &dir,
        &out,
        &mut lines,
        1,
        Call::ReadFile {
            path: dir.join("nope"),
        },
    )
    .await;
    assert!(reply.is_err());
    let _ = fs::remove_dir_all(&dir);
}

/// A folder missing on the host is said to be, not listed as empty.
#[tokio::test]
async fn a_missing_folder_is_refused() {
    let dir = scratch("missing-dir");
    let (out, mut lines) = mpsc::unbounded_channel();
    let path = dir.join("nope");
    let reply = ask_file(&dir, &out, &mut lines, 1, Call::ReadDir { path }).await;
    assert!(reply.is_err(), "{reply:?}");
    let _ = fs::remove_dir_all(&dir);
}

/// serde cannot write a time before 1970; the laptop must hear an error,
/// not wait out its timeout.
#[tokio::test]
async fn a_file_from_before_1970_is_refused_not_dropped() {
    let dir = scratch("old");
    let file = dir.join("old.txt");
    fs::write(&file, "x").unwrap();
    let times = fs::FileTimes::new().set_modified(UNIX_EPOCH - Duration::from_secs(10));
    File::options()
        .write(true)
        .open(&file)
        .unwrap()
        .set_times(times)
        .unwrap();
    let (out, mut lines) = mpsc::unbounded_channel();
    let reply = ask_file(&dir, &out, &mut lines, 1, Call::ReadFile { path: file }).await;
    assert!(reply.is_err(), "{reply:?}");
    let _ = fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn git_status_of_a_plain_folder_is_none() {
    let dir = scratch("git");
    let (out, mut lines) = mpsc::unbounded_channel();
    let reply = ask_file(
        &dir,
        &out,
        &mut lines,
        1,
        Call::GitStatus { root: dir.clone() },
    )
    .await;
    assert!(matches!(reply, Ok(Reply::Git(None))), "{reply:?}");
    let _ = fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn a_slow_call_holds_up_no_other() {
    let dir = scratch("both");
    let (out, mut lines) = mpsc::unbounded_channel();
    for (id, call) in [(1, Call::GitStatus { root: dir.clone() }), (2, Call::List)] {
        let ask = serde_json::to_string(&Question { id, call }).unwrap();
        answer(&hub_in(&dir), &terms_in(), &Build("b1".into()), &out, &ask);
    }
    let mut ids = Vec::new();
    for _ in 0..2 {
        let line = tokio::time::timeout(Duration::from_secs(5), lines.recv()).await;
        let Ok(Some(FromHost::Answer { id, .. })) = line else {
            panic!("not an answer: {line:?}");
        };
        ids.push(id);
    }
    ids.sort();
    assert_eq!(ids, [1, 2]);
    let _ = fs::remove_dir_all(&dir);
}
