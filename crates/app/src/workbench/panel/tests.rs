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
    assert_eq!(markdown.borrow().len(), 1);
    // Nor can it be asked for.
    panel.update(cx, |panel, cx| panel.set_mode(MARKDOWN.id, cx));
    panel.read_with(cx, |panel, _| assert_eq!(panel.mode(), EDITOR.id));

    panel.update(cx, |panel, cx| {
        panel.set_root(root.clone(), Disk::Local, cx)
    });
    panel.read_with(cx, |panel, _| {
        assert_eq!(offered_ids(panel), vec![EDITOR.id, MARKDOWN.id]);
        assert!(!panel.on_host);
    });
}

/// A host's attach made again while its project stays on screen, and a
/// folder here on the same path, each reach the modes with their own disk.
#[gpui::test]
fn the_same_root_on_another_disk_reaches_the_modes(cx: &mut TestAppContext) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let (panel, editor, _) = panel(cx);
    let root = PathBuf::from("/srv/app");
    let host = Some("vps".to_string());
    for disk in [host_disk(&rt), host_disk(&rt), Disk::Local, Disk::Local] {
        panel.update(cx, |panel, cx| panel.set_root(root.clone(), disk, cx));
    }
    let seen: Vec<_> = editor.borrow().iter().map(|(_, h)| h.clone()).collect();
    // The second folder here is the one already on screen: nothing new.
    assert_eq!(seen, vec![host.clone(), host, None]);
}

#[gpui::test]
fn a_refusal_says_why_and_a_root_lifts_it(cx: &mut TestAppContext) {
    let (panel, _, _) = panel(cx);
    panel.update(cx, |panel, cx| {
        panel.refuse(Some("Attaching to vps…".into()), cx)
    });
    panel.read_with(cx, |panel, _| {
        assert_eq!(panel.refused.as_deref(), Some("Attaching to vps…"))
    });
    panel.update(cx, |panel, cx| {
        panel.set_root(PathBuf::from("/srv/app"), Disk::Local, cx)
    });
    panel.read_with(cx, |panel, _| assert!(panel.refused.is_none()));
}
