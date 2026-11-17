use super::*;

/// An empty directory of its own for one test.
fn dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("kept-test-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn kept() -> Kept {
    Kept {
        workers: vec![KeptWorker {
            handle: "w-1".into(),
            conversation: "c1".into(),
            agent: "a".into(),
            folder: PathBuf::from("/repo"),
            dirs: vec![PathBuf::from("/x")],
            name: Some("fix".into()),
            turn: Some(2),
            text: Some("go".into()),
            person: true,
            unended: true,
        }],
        closed: vec![KeptClosed {
            handle: "w-2".into(),
            why: Some("its folder is gone".into()),
        }],
    }
}

#[test]
fn a_kept_file_reads_back_as_written() {
    let dir = dir("round-trip");
    write_blocking(&dir, &kept()).unwrap();
    assert_eq!(read_blocking(&dir), kept());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_missing_file_keeps_nothing() {
    let dir = dir("missing");
    assert_eq!(read_blocking(&dir), Kept::default());
    assert_eq!(read_blocking(&dir.join("nowhere")), Kept::default());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_malformed_file_keeps_nothing() {
    let dir = dir("malformed");
    std::fs::write(dir.join(FILE), "{not json").unwrap();
    assert_eq!(read_blocking(&dir), Kept::default());
    // The next write replaces it.
    write_blocking(&dir, &kept()).unwrap();
    assert_eq!(read_blocking(&dir), kept());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn writing_nothing_removes_the_file() {
    let dir = dir("nothing");
    write_blocking(&dir, &kept()).unwrap();
    write_blocking(&dir, &Kept::default()).unwrap();
    assert!(!dir.join(FILE).exists());
    write_blocking(&dir, &Kept::default()).unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_write_leaves_no_temporary_file() {
    let dir = dir("no-temporary");
    write_blocking(&dir, &kept()).unwrap();
    let names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, [FILE]);
    let _ = std::fs::remove_dir_all(dir);
}

/// A file written before whether the lead's turn ended was kept still
/// reads whole, that turn read as ended.
#[test]
fn a_file_kept_before_unended_was_kept_reads_whole() {
    let dir = dir("before-unended");
    let text = r#"{"workers":[{"handle":"w-1","conversation":"c1","agent":"a",
        "folder":"/repo","dirs":["/x"],"name":"fix","turn":2,"text":"go","person":true}],
        "closed":[{"handle":"w-2","why":"its folder is gone"}]}"#;
    std::fs::write(dir.join(FILE), text).unwrap();
    let mut expected = kept();
    expected.workers[0].unended = false;
    assert_eq!(read_blocking(&dir), expected);
    let _ = std::fs::remove_dir_all(dir);
}
