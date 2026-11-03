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
        let mut text = serde_json::to_string(&Ask { id, call }).unwrap();
        text.push('\n');
        self.write.write_all(text.as_bytes()).await.unwrap();
    }

    /// The next line, or `None` once the service closed this stream.
    async fn line(&mut self) -> Option<Line> {
        let next = tokio::time::timeout(Duration::from_secs(5), self.lines.next_line());
        let text = next.await.expect("a line in time").unwrap()?;
        Some(serde_json::from_str(&text).unwrap())
    }
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
    let service = tokio::spawn(accept(listener, hub, "b1".into()));

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
    let Some(Line::Answer {
        id: 1,
        result: Ok(Reply::Opened { session }),
    }) = first.line().await
    else {
        panic!("not opened");
    };
    let journal = dir.join("sessions").join(&session);
    for _ in 0..500 {
        if onehand_core::host::journal::read(&journal).unwrap().len() == 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    first.ask(2, Call::Watch { session }).await;
    assert!(matches!(
        first.line().await,
        Some(Line::Entry { seq: 0, .. })
    ));
    assert!(matches!(
        first.line().await,
        Some(Line::Entry { seq: 1, .. })
    ));
    assert!(matches!(
        first.line().await,
        Some(Line::Answer {
            id: 2,
            result: Ok(Reply::Done)
        })
    ));

    let mut second = Laptop::new(&socket).await;
    assert!(matches!(first.line().await, Some(Line::Kicked)));
    assert!(first.line().await.is_none());
    second.ask(1, Call::Hello { build: "b1".into() }).await;
    assert!(matches!(
        second.line().await,
        Some(Line::Answer { id: 1, result: Ok(Reply::Welcome { build, busy }) })
            if build == "b1" && busy.is_empty()
    ));
    service.abort();
    let _ = fs::remove_dir_all(&dir);
}

/// `Shutdown` itself ends the process, so only its answer is tested here.
#[tokio::test]
async fn shutdown_answers_done_and_asks_for_the_exit() {
    let dir = std::env::temp_dir().join(format!("onehand-shutdown-{}", std::process::id()));
    let (out, mut lines) = mpsc::unbounded_channel();
    let ask = serde_json::to_string(&Ask {
        id: 7,
        call: Call::Shutdown,
    })
    .unwrap();
    assert!(answer(&hub_in(&dir), "b1", &out, &ask));
    assert!(matches!(
        lines.try_recv(),
        Ok(Line::Answer {
            id: 7,
            result: Ok(Reply::Done)
        })
    ));
}
