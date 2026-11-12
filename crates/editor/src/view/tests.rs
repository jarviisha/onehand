//! The Editor's carrier against its contract: a read's answer becomes a
//! buffer once, a write goes through the disk its buffer was read from and its
//! answer reaches the drive, a failure is said once in the window, and a save
//! asked while one writes waits for it.

use super::*;
use gpui::TestAppContext;
use gpui_component::Root;
use onehand_core::disk::HostDisk;
use onehand_core::host::client::HostConn;

/// The view in a window of its own.
fn mounted(cx: &mut TestAppContext) -> (Entity<EditorView>, gpui::WindowHandle<Root>) {
    cx.update(gpui_component::init);
    let view = cx.update(EditorView::new);
    let window = cx.add_window({
        let view = view.clone();
        |window, cx| Root::new(view, window, cx)
    });
    (view, window)
}

/// `path` read in `root` as holding `mine` at `mtime`, as a read's answer
/// lands, its buffer built in the view's window, and typed into: its tab's uid.
fn edited(
    cx: &mut TestAppContext,
    view: &Entity<EditorView>,
    window: gpui::WindowHandle<Root>,
    root: &Path,
    path: &Path,
    mtime: Option<std::time::SystemTime>,
) -> u64 {
    view.update(cx, |view, cx| {
        view.window = Some(window.into());
        view.landed(
            Input::Read {
                root: root.to_path_buf(),
                path: path.to_path_buf(),
                read: Ok(("mine".into(), mtime)),
                uid: super::next_uid(),
            },
            None,
            cx,
        );
    });
    cx.run_until_parked();
    view.update(cx, |view, _| {
        let uid = view.drive.tabs(root).unwrap().files().last().unwrap().uid;
        assert!(view.buffers.contains_key(&uid), "no buffer built");
        view.drive.typed(root, uid);
        uid
    })
}

/// A folder of its own for one test, holding `a.rs` as `theirs`.
fn folder(name: &str) -> (PathBuf, PathBuf) {
    let root = std::env::temp_dir().join(format!("onehand-editor-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let file = root.join("a.rs");
    std::fs::write(&file, "theirs").unwrap();
    (root, file)
}

fn notes(cx: &mut TestAppContext, window: gpui::WindowHandle<Root>) -> usize {
    cx.update_window(window.into(), |_, window, cx| {
        window.notifications(cx).len()
    })
    .unwrap()
}

fn dirty(view: &Entity<EditorView>, root: &Path, cx: &mut TestAppContext) -> bool {
    view.read_with(cx, |view, _| view.drive.unsaved(root) > 0)
}

fn typed(
    view: &Entity<EditorView>,
    uid: u64,
    text: &str,
    window: gpui::WindowHandle<Root>,
    cx: &mut TestAppContext,
) {
    let state = view.read_with(cx, |view, _| view.buffers[&uid].0.clone());
    window
        .update(cx, |_, window, cx| {
            state.update(cx, |state, cx| {
                state.set_value(text.to_string(), window, cx)
            })
        })
        .unwrap();
}

#[gpui::test]
fn a_buffer_saves_through_its_roots_disk(cx: &mut TestAppContext) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let host = Disk::Host(HostDisk::new(HostConn::closed("vps"), rt.handle().clone()));
    let (local, _) = folder("host");
    // A folder this machine could write, so a save that took the wrong disk
    // would leave a file behind.
    let remote = local.join("on-host");
    std::fs::create_dir_all(&remote).unwrap();

    let (view, window) = mounted(cx);
    view.update(cx, |view, cx| {
        view.set_root(&local, &Disk::Local, cx);
        view.set_root(&remote, &host, cx);
    });
    // The host's root holds a file of the same name as the local one, which
    // its save must not reach.
    edited(cx, &view, window, &remote, &remote.join("a.rs"), None);
    view.update(cx, |view, cx| view.save_active(cx));
    cx.run_until_parked();

    // Said once, as a failure toast rather than a standing line.
    assert_eq!(notes(cx, window), 1);
    view.read_with(cx, |view, _| assert_eq!(view.drive.standing(&remote), None));
    assert!(dirty(&view, &remote, cx));
    assert_eq!(
        std::fs::read_to_string(local.join("a.rs")).unwrap(),
        "theirs"
    );
    assert!(!remote.join("a.rs").exists());
    std::fs::remove_dir_all(&local).unwrap();
}

/// A local and a host folder can share a path: a buffer read here is
/// written back here even once the root's disk is the host's.
#[gpui::test]
fn a_buffer_saves_through_the_disk_it_was_read_from(cx: &mut TestAppContext) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let host = Disk::Host(HostDisk::new(HostConn::closed("vps"), rt.handle().clone()));
    let (root, file) = folder("read");

    let (view, window) = mounted(cx);
    view.update(cx, |view, cx| view.set_root(&root, &Disk::Local, cx));
    edited(cx, &view, window, &root, &file, None);
    view.update(cx, |view, cx| {
        // The host's folder of the same path takes the root over.
        view.set_root(&root, &host, cx);
        view.save_active(cx);
    });
    cx.run_until_parked();

    assert_eq!(std::fs::read_to_string(&file).unwrap(), "mine");
    assert_eq!(notes(cx, window), 0);
    assert!(!dirty(&view, &root, cx));
    std::fs::remove_dir_all(&root).unwrap();
}

/// A file pressed in the tree is read once off the UI loop and opens as one
/// tab, whose buffer holds what was read.
#[gpui::test]
fn an_open_reads_once_and_builds_one_buffer(cx: &mut TestAppContext) {
    let (root, file) = folder("open");
    let (view, window) = mounted(cx);
    window
        .update(cx, |_, window, cx| {
            view.update(cx, |view, cx| {
                view.set_root(&root, &Disk::Local, cx);
                assert!(view.open(&file, window, cx));
                assert!(view.open(&file, window, cx));
            })
        })
        .unwrap();
    cx.run_until_parked();
    view.read_with(cx, |view, cx| {
        assert_eq!(view.drive.tabs(&root).unwrap().files().len(), 1);
        assert_eq!(view.buffers.len(), 1);
        let (state, _) = view.buffers.values().next().unwrap();
        assert_eq!(state.read(cx).text().to_string(), "theirs");
        assert_eq!(view.drive.opening(), None);
    });
    std::fs::remove_dir_all(&root).unwrap();
}

/// A second save asked for while the first is still writing waits for it,
/// then writes what was typed since: it is not a conflict with our own write.
#[gpui::test]
fn a_save_asked_while_one_writes_runs_after_it_with_the_latest_text(cx: &mut TestAppContext) {
    let (root, file) = folder("again");
    let (view, window) = mounted(cx);
    view.update(cx, |view, cx| view.set_root(&root, &Disk::Local, cx));
    // Read as the file stands, so a second write carrying this time after the
    // first had landed would be a conflict.
    let seen = std::fs::metadata(&file).unwrap().modified().ok();
    let uid = edited(cx, &view, window, &root, &file, seen);
    view.update(cx, |view, cx| view.save_active(cx));
    typed(&view, uid, "mine, and more", window, cx);
    view.update(cx, |view, cx| view.save_active(cx));
    cx.run_until_parked();

    assert_eq!(std::fs::read_to_string(&file).unwrap(), "mine, and more");
    view.read_with(cx, |view, _| assert_eq!(view.drive.standing(&root), None));
    assert!(!dirty(&view, &root, cx));
    std::fs::remove_dir_all(&root).unwrap();
}

/// A folder whose code is elsewhere for now is not written, and its tab
/// keeps its edits.
#[gpui::test]
fn a_locked_folder_is_not_written(cx: &mut TestAppContext) {
    let (root, file) = folder("locked");
    let (view, window) = mounted(cx);
    view.update(cx, |view, cx| {
        view.set_root(&root, &Disk::Local, cx);
        view.set_locked(&HashMap::from([(root.clone(), "on vps".into())]), cx);
    });
    edited(cx, &view, window, &root, &file, None);
    view.update(cx, |view, cx| view.save_active(cx));
    cx.run_until_parked();

    assert_eq!(std::fs::read_to_string(&file).unwrap(), "theirs");
    assert_eq!(view.read_with(cx, |view, _| view.unsaved(&root)), 1);
    std::fs::remove_dir_all(&root).unwrap();
}

/// A file written by somebody else since it was read is not overwritten:
/// the line under the body says so, and the next save overwrites it.
#[gpui::test]
fn a_file_changed_on_disk_is_a_conflict_until_saved_again(cx: &mut TestAppContext) {
    let (root, file) = folder("conflict");
    let (view, window) = mounted(cx);
    view.update(cx, |view, cx| view.set_root(&root, &Disk::Local, cx));
    // Read long before the file was last written.
    edited(
        cx,
        &view,
        window,
        &root,
        &file,
        Some(std::time::SystemTime::UNIX_EPOCH),
    );
    view.update(cx, |view, cx| view.save_active(cx));
    cx.run_until_parked();

    assert_eq!(std::fs::read_to_string(&file).unwrap(), "theirs");
    view.read_with(cx, |view, _| {
        let said = view
            .drive
            .standing(&root)
            .map(standing_line)
            .unwrap_or_default();
        assert!(said.contains("changed on disk"), "{said}");
    });
    assert!(dirty(&view, &root, cx));

    view.update(cx, |view, cx| view.save_active(cx));
    cx.run_until_parked();
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "mine");
    view.read_with(cx, |view, _| assert_eq!(view.drive.standing(&root), None));
    assert!(!dirty(&view, &root, cx));
    std::fs::remove_dir_all(&root).unwrap();
}

/// What a removal would throw away is counted per project, and forgetting a
/// project drops its buffers with it.
#[gpui::test]
fn unsaved_edits_are_counted_per_project_and_go_with_it(cx: &mut TestAppContext) {
    let (root, file) = folder("forget");
    let (other, other_file) = folder("forget-other");
    let (view, window) = mounted(cx);
    view.update(cx, |view, cx| {
        view.set_root(&root, &Disk::Local, cx);
        view.set_root(&other, &Disk::Local, cx);
    });
    edited(cx, &view, window, &root, &file, None);
    edited(cx, &view, window, &other, &other_file, None);
    view.update(cx, |view, cx| {
        assert_eq!(view.unsaved(&root), 1);
        assert_eq!(view.unsaved(&other), 1);
        view.forget_root(&root, cx);
        assert_eq!(view.unsaved(&root), 0);
        assert_eq!(view.unsaved(&other), 1);
        assert_eq!(view.buffers.len(), 1);
    });
    std::fs::remove_dir_all(&root).unwrap();
    std::fs::remove_dir_all(&other).unwrap();
}
