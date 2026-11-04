use super::*;
use gpui::{AnyView, TestAppContext};
use onehand_core::disk::HostDisk;
use onehand_core::host::client::HostConn;
use std::cell::RefCell;

struct Body;

impl Render for Body {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

/// A mode that writes down every root it is pointed at, and on which disk.
struct Stub {
    spec: WorkbenchModeSpec,
    view: AnyView,
    roots: Roots,
}

impl WorkbenchMode for Stub {
    fn spec(&self) -> WorkbenchModeSpec {
        self.spec
    }

    fn view(&self) -> AnyView {
        self.view.clone()
    }

    fn set_root(&mut self, root: &Path, disk: &Disk, _: &mut App) {
        let host = disk.host().map(str::to_string);
        self.roots.borrow_mut().push((root.to_path_buf(), host));
    }

    fn forget_root(&mut self, root: &Path, _: &mut App) {
        let forgot = PathBuf::from("forgot").join(root.strip_prefix("/").unwrap_or(root));
        self.roots.borrow_mut().push((forgot, None));
    }

    /// A rescan written down as a root named so.
    fn handle(&mut self, request: &Request<'_>, _: &mut App) -> bool {
        if let Request::Rescan = request {
            self.roots
                .borrow_mut()
                .push((PathBuf::from("rescan"), None));
        }
        false
    }
}

const EDITOR: WorkbenchModeSpec =
    WorkbenchModeSpec::element(ModeId::new("editor"), "Editor").on_host();
const MARKDOWN: WorkbenchModeSpec = WorkbenchModeSpec::element(ModeId::new("markdown"), "Markdown");

type Roots = Rc<RefCell<Vec<(PathBuf, Option<String>)>>>;

/// A panel holding Editor and Markdown, and what each was pointed at.
fn panel(cx: &mut TestAppContext) -> (Entity<Workbench>, Roots, Roots) {
    let (editor, markdown) = (Roots::default(), Roots::default());
    let (e, m) = (editor.clone(), markdown.clone());
    let panel = cx.update(|cx| {
        Workbench::new(
            move |_, _, cx| {
                let view: AnyView = cx.new(|_| Body).into();
                let stub = |spec, roots| -> Box<dyn WorkbenchMode> {
                    Box::new(Stub {
                        spec,
                        view: view.clone(),
                        roots,
                    })
                };
                vec![stub(EDITOR, e), stub(MARKDOWN, m)]
            },
            cx,
        )
    });
    (panel, editor, markdown)
}

fn host_disk(rt: &tokio::runtime::Runtime) -> Disk {
    Disk::Host(HostDisk::new(HostConn::closed("vps"), rt.handle().clone()))
}

fn offered_ids(panel: &Workbench) -> Vec<ModeId> {
    panel
        .modes
        .iter()
        .map(|mode| mode.spec())
        .filter(|spec| panel.offers(*spec))
        .map(|spec| spec.id)
        .collect()
}

#[gpui::test]
fn a_host_root_offers_only_modes_that_run_there(cx: &mut TestAppContext) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let (panel, _, markdown) = panel(cx);
    let root = PathBuf::from("/srv/app");
    panel.update(cx, |panel, cx| {
        panel.set_root(root.clone(), Disk::Local, cx);
        panel.set_mode(MARKDOWN.id, cx);
    });

    panel.update(cx, |panel, cx| {
        panel.set_root(root.clone(), host_disk(&rt), cx)
    });
    panel.read_with(cx, |panel, _| {
        assert_eq!(panel.mode(), EDITOR.id);
        assert_eq!(offered_ids(panel), vec![EDITOR.id]);
    });
    // Markdown reads this machine's disk, so it is never pointed at the host.
    let pointed = markdown.borrow().iter().filter(|(r, _)| r == &root).count();
    assert_eq!(pointed, 1);
    // Nor can it be asked for.
    panel.update(cx, |panel, cx| panel.set_mode(MARKDOWN.id, cx));
    panel.read_with(cx, |panel, _| assert_eq!(panel.mode(), EDITOR.id));

    panel.update(cx, |panel, cx| {
        panel.set_root(root.clone(), Disk::Local, cx)
    });
    panel.read_with(cx, |panel, _| {
        assert_eq!(offered_ids(panel), vec![EDITOR.id, MARKDOWN.id]);
        assert!(!panel.on_host());
    });
}

/// A host's attach made again while its project stays on screen, and a
/// folder here on the same path, each reach the modes with their own disk,
/// and the modes read the root again through it.
#[gpui::test]
fn the_same_root_on_another_disk_reaches_the_modes(cx: &mut TestAppContext) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let (panel, editor, _) = panel(cx);
    let root = PathBuf::from("/srv/app");
    for disk in [host_disk(&rt), host_disk(&rt), Disk::Local, Disk::Local] {
        panel.update(cx, |panel, cx| panel.set_root(root.clone(), disk, cx));
    }
    let seen: Vec<_> = editor
        .borrow()
        .iter()
        .map(|(root, host)| format!("{} {host:?}", root.display()))
        .collect();
    // The second folder here is the one already on screen: nothing new.
    let (vps, rescan) = ("/srv/app Some(\"vps\")", "rescan None");
    assert_eq!(seen, [vps, vps, rescan, "/srv/app None", rescan]);
}

/// An attach that ended under a tree on screen: the panel says it is lost,
/// stops saying so while the attach is made again, and the new attach reads
/// the tree again rather than keep what the dead one said.
#[gpui::test]
fn a_host_attach_made_again_reads_the_tree_again(cx: &mut TestAppContext) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let (panel, editor, _) = panel(cx);
    let root = PathBuf::from("/srv/app");
    panel.update(cx, |panel, cx| {
        panel.set_root(root.clone(), host_disk(&rt), cx)
    });
    assert!(panel.update(cx, |panel, cx| panel.attach_again(cx)));

    panel.update(cx, |panel, cx| {
        panel.refuse(Some(Refusal::Waiting("Attaching to vps…".into())), cx)
    });
    // Attaching already: nothing to attach again for.
    assert!(!panel.update(cx, |panel, cx| panel.attach_again(cx)));

    editor.borrow_mut().clear();
    panel.update(cx, |panel, cx| {
        panel.set_root(root.clone(), host_disk(&rt), cx)
    });
    let seen: Vec<_> = editor.borrow().iter().map(|(r, _)| r.clone()).collect();
    assert_eq!(seen, [root, PathBuf::from("rescan")]);
    panel.read_with(cx, |panel, _| assert!(panel.refused.is_none()));
}

/// A folder here is never lost, whatever happened to a host.
#[gpui::test]
fn a_local_root_is_never_lost(cx: &mut TestAppContext) {
    let (panel, _, _) = panel(cx);
    panel.update(cx, |panel, cx| {
        panel.set_root(PathBuf::from("/srv/app"), Disk::Local, cx)
    });
    assert!(!panel.update(cx, |panel, cx| panel.attach_again(cx)));
}

/// An attach the host closed because another machine attached is not made
/// again: the panel says who holds the host, and *Try again* is the way back.
#[gpui::test]
fn a_host_taken_by_another_machine_is_not_attached_again(cx: &mut TestAppContext) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let (panel, _, _) = panel(cx);
    let taken = Disk::Host(HostDisk::new(
        HostConn::taken_from("vps"),
        rt.handle().clone(),
    ));
    panel.update(cx, |panel, cx| {
        panel.set_root(PathBuf::from("/srv/app"), taken, cx)
    });
    assert!(!panel.update(cx, |panel, cx| panel.attach_again(cx)));
    panel.read_with(cx, |panel, _| {
        assert_eq!(
            panel.refused,
            Some(Refusal::Failed(
                "vps was attached from another machine".into()
            ))
        )
    });
    // Standing, it asks for nothing more until a person answers it.
    assert!(!panel.update(cx, |panel, cx| panel.attach_again(cx)));
}

#[gpui::test]
fn a_refusal_says_why_and_a_root_lifts_it(cx: &mut TestAppContext) {
    let (panel, _, _) = panel(cx);
    panel.update(cx, |panel, cx| {
        panel.refuse(Some(Refusal::Failed("Could not attach to vps".into())), cx)
    });
    panel.read_with(cx, |panel, _| {
        assert_eq!(
            panel.refused,
            Some(Refusal::Failed("Could not attach to vps".into()))
        )
    });
    panel.update(cx, |panel, cx| {
        panel.set_root(PathBuf::from("/srv/app"), Disk::Local, cx)
    });
    panel.read_with(cx, |panel, _| assert!(panel.refused.is_none()));
}

/// Removing a host's folder forgets it only in the modes offered there:
/// what the rest hold at that path is a folder here, which stays.
#[gpui::test]
fn forgetting_a_host_root_reaches_only_modes_that_run_there(cx: &mut TestAppContext) {
    let (panel, editor, markdown) = panel(cx);
    let root = PathBuf::from("/srv/app");
    let forgot = PathBuf::from("forgot/srv/app");
    let forgotten = |roots: &Roots| roots.borrow().iter().any(|(r, _)| r == &forgot);

    panel.update(cx, |panel, cx| panel.forget_root(&root, true, cx));
    assert!(forgotten(&editor));
    assert!(!forgotten(&markdown));

    panel.update(cx, |panel, cx| panel.forget_root(&root, false, cx));
    assert!(forgotten(&markdown));
}
