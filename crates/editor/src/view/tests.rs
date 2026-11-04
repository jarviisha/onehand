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
