use super::*;

#[test]
fn local_reads_and_saves_like_the_editor() {
    let dir = std::env::temp_dir().join(format!("onehand-disk-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("a.txt");
    let disk = Disk::Local;

    let SaveOutcome::Saved { mtime, .. } = disk.save_file(&path, "one".into(), None) else {
        panic!("the first save failed");
    };
    assert_eq!(disk.read_file(&path).unwrap(), ("one".into(), mtime));

    // Someone else writes it; a save that still holds the old time must not
    // overwrite them.
    std::thread::sleep(std::time::Duration::from_millis(10));
    std::fs::write(&path, "theirs").unwrap();
    let stale = disk.save_file(&path, "mine".into(), mtime);
    assert!(matches!(stale, SaveOutcome::Conflict { .. }), "{stale:?}");
    assert_eq!(disk.read_file(&path).unwrap().0, "theirs");
    assert_eq!(disk.host(), None);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_host_disk_over_a_closed_connection_says_so() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let disk = Disk::Host(HostDisk::new(HostConn::closed("vps"), rt.handle().clone()));
    std::thread::spawn(move || {
        let err = disk.read_dir(Path::new("/srv/app")).unwrap_err();
        assert!(err.contains("on vps"), "{err}");
        assert_eq!(disk.git_status(Path::new("/srv/app")), None);
        let saved = disk.save_file(Path::new("/srv/app/a.rs"), "x".into(), None);
        assert!(
            matches!(saved, SaveOutcome::Failed(ref why) if why.starts_with("On vps: ")),
            "{saved:?}"
        );
        assert_eq!(disk.host(), Some("vps"));
    })
    .join()
    .unwrap();
}
