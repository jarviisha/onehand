use super::*;
use crate::buffers::new_buffer;
use gpui::TestAppContext;
use gpui_component::Root;
use onehand_core::disk::HostDisk;
use onehand_core::host::client::HostConn;

/// Give `root` a dirty tab on `path`, as typing into a file opened through
/// the root's disk would.
fn edited(view: &mut EditorView, root: &Path, path: &Path, window: &mut Window, cx: &mut App) {
    let disk = view.disks[root].clone();
    let uid = next_buffer_uid();
    let state = new_buffer(path, "mine", window, cx);
    let watch = cx.observe(&state, |_, _| {});
    let buffers = view.buffers.entry(root.to_path_buf()).or_default();
    buffers.tabs.open(path.to_path_buf(), None, uid);
    buffers.tabs.files.last_mut().unwrap().dirty = true;
    buffers.insert(uid, state, watch, disk);
}

#[gpui::test]
fn a_buffer_saves_through_its_roots_disk(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let host = Disk::Host(HostDisk::new(HostConn::closed("vps"), rt.handle().clone()));
    let local = std::env::temp_dir().join(format!("onehand-editor-{}", std::process::id()));
    std::fs::create_dir_all(&local).unwrap();
    std::fs::write(local.join("a.rs"), "theirs").unwrap();
    // A folder this machine could write, so a save that took the wrong disk
    // would leave a file behind.
    let remote = local.join("on-host");
    std::fs::create_dir_all(&remote).unwrap();

    let view = cx.update(EditorView::new);
    let window = cx.add_window({
        let view = view.clone();
        |window, cx| Root::new(view, window, cx)
    });
    window
        .update(cx, |_, window, cx| {
            view.update(cx, |view, cx| {
                view.set_root(&local, &Disk::Local, cx);
                view.set_root(&remote, &host, cx);
                // The host's root holds a file of the same name as the
                // local one, which its save must not reach.
                edited(view, &remote, &remote.join("a.rs"), window, cx);
                view.window = Some(window.window_handle());
                view.save_active(cx);
            })
        })
        .unwrap();
    cx.run_until_parked();

    // Through the bare handle: the typed one leases the root the toasts are
    // read from.
    cx.update_window(window.into(), |_, window, cx| {
        // Said once, as a failure toast rather than a standing line.
        assert_eq!(window.notifications(cx).len(), 1);
        let view = view.read(cx);
        assert_eq!(view.status, None);
        assert!(view.buffers[&remote].tabs.files[0].dirty);
        let failed = view.disks[&remote].save_file(&remote.join("a.rs"), "x".into(), None);
        assert!(
            matches!(failed, SaveOutcome::Failed(ref why) if why.contains("On vps")),
            "{failed:?}"
        );
    })
    .unwrap();
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
    cx.update(gpui_component::init);
    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let host = Disk::Host(HostDisk::new(HostConn::closed("vps"), rt.handle().clone()));
    let root = std::env::temp_dir().join(format!("onehand-editor-read-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let file = root.join("a.rs");
    std::fs::write(&file, "theirs").unwrap();

    let view = cx.update(EditorView::new);
    let window = cx.add_window({
        let view = view.clone();
        |window, cx| Root::new(view, window, cx)
    });
    window
        .update(cx, |_, window, cx| {
            view.update(cx, |view, cx| {
                view.set_root(&root, &Disk::Local, cx);
                edited(view, &root, &file, window, cx);
                // The host's folder of the same path takes the root over.
                view.set_root(&root, &host, cx);
                view.window = Some(window.window_handle());
                view.save_active(cx);
            })
        })
        .unwrap();
    cx.run_until_parked();

    assert_eq!(std::fs::read_to_string(&file).unwrap(), "mine");
    cx.update_window(window.into(), |_, window, cx| {
        assert!(window.notifications(cx).is_empty());
        assert!(!view.read(cx).buffers[&root].tabs.files[0].dirty);
    })
    .unwrap();
    std::fs::remove_dir_all(&root).unwrap();
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

/// The text of `root`'s first tab.
fn typed(view: &mut EditorView, root: &Path, text: &str, window: &mut Window, cx: &mut App) {
    let uid = view.buffers[root].tabs.files[0].uid;
    let state = view.buffers[root].buffer(uid).unwrap().clone();
    state.update(cx, |state, cx| {
        state.set_value(text.to_string(), window, cx)
    });
}

/// A second save asked for while the first is still writing waits for it,
/// then writes what was typed since: it is not a conflict with our own write.
#[gpui::test]
fn a_save_asked_while_one_writes_runs_after_it_with_the_latest_text(cx: &mut TestAppContext) {
    let (root, file) = folder("again");
    let (view, window) = mounted(cx);
    window
        .update(cx, |_, window, cx| {
            view.update(cx, |view, cx| {
                view.set_root(&root, &Disk::Local, cx);
                edited(view, &root, &file, window, cx);
                // Read as the file stands, so a second write carrying this
                // time after the first had landed would be a conflict.
                view.buffers.get_mut(&root).unwrap().tabs.files[0].disk_mtime =
                    std::fs::metadata(&file).unwrap().modified().ok();
                view.save_active(cx);
                typed(view, &root, "mine, and more", window, cx);
                view.save_active(cx);
            })
        })
        .unwrap();
    cx.run_until_parked();

    assert_eq!(std::fs::read_to_string(&file).unwrap(), "mine, and more");
    view.read_with(cx, |view, _| {
        assert_eq!(view.status, None);
        assert!(view.saving.is_empty());
        assert!(!view.buffers[&root].tabs.files[0].dirty);
    });
    std::fs::remove_dir_all(&root).unwrap();
}

/// A folder whose code is elsewhere for now is not written, and its tab
/// keeps its edits.
#[gpui::test]
fn a_locked_folder_is_not_written(cx: &mut TestAppContext) {
    let (root, file) = folder("locked");
    let (view, window) = mounted(cx);
    window
        .update(cx, |_, window, cx| {
            view.update(cx, |view, cx| {
                view.set_root(&root, &Disk::Local, cx);
                view.set_locked(&HashMap::from([(root.clone(), "on vps".into())]), cx);
                edited(view, &root, &file, window, cx);
                view.save_active(cx);
            })
        })
        .unwrap();
    cx.run_until_parked();

    assert_eq!(std::fs::read_to_string(&file).unwrap(), "theirs");
    view.read_with(cx, |view, _| {
        assert!(view.buffers[&root].tabs.files[0].dirty);
        assert_eq!(view.unsaved(&root), 1);
    });
    std::fs::remove_dir_all(&root).unwrap();
}

/// A file written by somebody else since it was read is not overwritten:
/// the line under the body says so, and the next save overwrites it.
#[gpui::test]
fn a_file_changed_on_disk_is_a_conflict_until_saved_again(cx: &mut TestAppContext) {
    let (root, file) = folder("conflict");
    let (view, window) = mounted(cx);
    window
        .update(cx, |_, window, cx| {
            view.update(cx, |view, cx| {
                view.set_root(&root, &Disk::Local, cx);
                edited(view, &root, &file, window, cx);
                // Read long before the file was last written.
                view.buffers.get_mut(&root).unwrap().tabs.files[0].disk_mtime =
                    Some(std::time::SystemTime::UNIX_EPOCH);
                view.save_active(cx);
            })
        })
        .unwrap();
    cx.run_until_parked();

    assert_eq!(std::fs::read_to_string(&file).unwrap(), "theirs");
    view.read_with(cx, |view, _| {
        let said = view.status.clone().unwrap_or_default();
        assert!(said.contains("changed on disk"), "{said}");
        assert!(view.buffers[&root].tabs.files[0].dirty);
    });

    view.update(cx, |view, cx| view.save_active(cx));
    cx.run_until_parked();
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "mine");
    view.read_with(cx, |view, _| {
        assert_eq!(view.status, None);
        assert!(!view.buffers[&root].tabs.files[0].dirty);
    });
    std::fs::remove_dir_all(&root).unwrap();
}

/// What a removal would throw away is counted per project, and forgetting a
/// project drops its buffers with it.
#[gpui::test]
fn unsaved_edits_are_counted_per_project_and_go_with_it(cx: &mut TestAppContext) {
    let (root, file) = folder("forget");
    let (other, other_file) = folder("forget-other");
    let (view, window) = mounted(cx);
    window
        .update(cx, |_, window, cx| {
            view.update(cx, |view, cx| {
                view.set_root(&root, &Disk::Local, cx);
                view.set_root(&other, &Disk::Local, cx);
                edited(view, &root, &file, window, cx);
                edited(view, &other, &other_file, window, cx);
            })
        })
        .unwrap();
    view.update(cx, |view, cx| {
        assert_eq!(view.unsaved(&root), 1);
        assert_eq!(view.unsaved(&other), 1);
        view.forget_root(&root, cx);
        assert_eq!(view.unsaved(&root), 0);
        assert_eq!(view.unsaved(&other), 1);
    });
    std::fs::remove_dir_all(&root).unwrap();
    std::fs::remove_dir_all(&other).unwrap();
}
