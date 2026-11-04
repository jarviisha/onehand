use super::*;
use gpui::TestAppContext;
use onehand_core::disk::HostDisk;
use onehand_core::host::client::HostConn;
use std::cell::RefCell;
use std::rc::Rc;

/// A view whose requests upward are written down rather than answered.
fn view(cx: &mut TestAppContext) -> (Entity<FilesView>, Rc<RefCell<Vec<PathBuf>>>) {
    let opened = Rc::new(RefCell::new(Vec::new()));
    let ask: Ask = Rc::new({
        let opened = opened.clone();
        move |request: &Request<'_>, _: &mut Window, _: &mut App| {
            if let Request::OpenFile(path) = request {
                opened.borrow_mut().push(path.to_path_buf());
            }
        }
    });
    (cx.update(|cx| FilesView::new(ask, cx)), opened)
}

#[gpui::test]
fn a_folder_the_disk_cannot_read_says_so(cx: &mut TestAppContext) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let disk = Disk::Host(HostDisk::new(HostConn::closed("vps"), rt.handle().clone()));
    let (view, _) = view(cx);
    let root = PathBuf::from("/srv/app");

    view.update(cx, |view, cx| view.set_root(&root, &disk, cx));
    cx.run_until_parked();

    view.read_with(cx, |view, _| {
        let why = &view.failed[&root][&root];
        assert!(why.contains("on vps"), "{why}");
        assert!(view.trees[&root].listings.is_empty());
    });
}

#[gpui::test]
fn a_row_opens_by_its_listed_kind(cx: &mut TestAppContext) {
    let (view, opened) = view(cx);
    // Nowhere on this machine: only the listing knows it is a folder.
    let root = PathBuf::from("/onehand-no-such-root");
    let folder = TreeEntry {
        path: root.join("src"),
        name: "src".into(),
        is_dir: true,
    };
    view.update(cx, |view, cx| view.set_root(&root, &Disk::Local, cx));
    let window = cx.add_empty_window();

    window.update(|window, cx| view.update(cx, |view, cx| view.click(&folder, window, cx)));

    view.read_with(window, |view, _| {
        assert!(view.trees[&root].expanded.contains(&folder.path));
    });
    assert!(opened.borrow().is_empty());
}

#[gpui::test]
fn a_folders_failure_outlasts_its_roots_listing(cx: &mut TestAppContext) {
    let (view, _) = view(cx);
    let root = PathBuf::from("/onehand-no-such-root");
    let folder = TreeEntry {
        path: root.join("src"),
        name: "src".into(),
        is_dir: true,
    };
    view.update(cx, |view, cx| view.set_root(&root, &Disk::Local, cx));
    cx.run_until_parked();
    let window = cx.add_empty_window();
    window.update(|window, cx| view.update(cx, |view, cx| view.click(&folder, window, cx)));
    window.run_until_parked();

    // A rescan's listings land in any order: the folder's failure first, then
    // the root's listing, which must not answer it.
    view.update(window, |view, cx| {
        view.listed(root.clone(), folder.path.clone(), Err("gone".into()), cx);
        view.listed(
            root.clone(),
            root.clone(),
            Ok((vec![folder.clone()], 0)),
            cx,
        );
        assert_eq!(view.failed[&root][&folder.path], "gone");
    });

    // Folding the folder away is the way out.
    window.update(|window, cx| view.update(cx, |view, cx| view.click(&folder, window, cx)));
    view.read_with(window, |view, _| assert!(view.failed.is_empty()));

    // So is the folder listing at last.
    view.update(window, |view, cx| {
        view.listed(root.clone(), folder.path.clone(), Err("gone".into()), cx);
        view.listed(root.clone(), folder.path.clone(), Ok((Vec::new(), 0)), cx);
        assert!(view.failed.is_empty());
    });
}
