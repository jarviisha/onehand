use super::*;
use gpui::{AnyView, TestAppContext};
use onehand_core::host::client::HostConn;
use std::cell::RefCell;

struct Body;

impl Render for Body {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

/// A mode that writes down every root it is pointed at, and on which disk,
/// and in `told` each time it is told it shows or asked to open a file;
/// it takes a file when `takes`.
struct Stub {
    spec: WorkbenchModeSpec,
    view: AnyView,
    roots: Roots,
    told: Told,
    takes: bool,
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

    fn open_file(&mut self, _: &Path, _: &mut Window, _: &mut App) -> bool {
        self.told.borrow_mut().push((self.spec.id, "open"));
        self.takes
    }

    /// A rescan written down as a root named so; being shown, in `told`.
    fn handle(&mut self, request: &Request<'_>, _: &mut App) -> bool {
        match request {
            Request::Rescan => self
                .roots
                .borrow_mut()
                .push((PathBuf::from("rescan"), None)),
            Request::Shown => self.told.borrow_mut().push((self.spec.id, "shown")),
            Request::Save
            | Request::Start
            | Request::Reap
            | Request::SetGit(_)
            | Request::SetStorage(_)
            | Request::SetFontSize(_)
            | Request::ShowIssue(_)
            | Request::AgentStarted(_)
            | Request::LiveConversations(_)
            | Request::IssueWork { .. }
            | Request::Locked(_)
            | Request::Offloaded(_) => {}
        }
        false
    }
}

const EDITOR: WorkbenchModeSpec =
    WorkbenchModeSpec::element(ModeId::new("editor"), "Editor").on_host();
const MARKDOWN: WorkbenchModeSpec = WorkbenchModeSpec::element(ModeId::new("markdown"), "Markdown");

type Roots = Rc<RefCell<Vec<(PathBuf, Option<String>)>>>;
type Told = Rc<RefCell<Vec<(ModeId, &'static str)>>>;

/// A panel holding Editor and Markdown, and what each was pointed at.
fn panel(cx: &mut TestAppContext) -> (Entity<Workbench>, Roots, Roots) {
    let (panel, editor, markdown, _) = panel_taking([false, false], cx);
    (panel, editor, markdown)
}

/// A panel holding Editor and Markdown, each taking a file as `takes` says,
/// what each was pointed at, and what each was told.
fn panel_taking(
    takes: [bool; 2],
    cx: &mut TestAppContext,
) -> (Entity<Workbench>, Roots, Roots, Told) {
    let (editor, markdown, told) = (Roots::default(), Roots::default(), Told::default());
    let (e, m, t) = (editor.clone(), markdown.clone(), told.clone());
    let panel = cx.update(|cx| {
        Workbench::new(
            move |_, _, cx| {
                let view: AnyView = cx.new(|_| Body).into();
                let stub = |spec, roots, takes| -> Box<dyn WorkbenchMode> {
                    Box::new(Stub {
                        spec,
                        view: view.clone(),
                        roots,
                        told: t.clone(),
                        takes,
                    })
                };
                vec![stub(EDITOR, e, takes[0]), stub(MARKDOWN, m, takes[1])]
            },
            cx,
        )
    });
    (panel, editor, markdown, told)
}

fn host_disk() -> Disk {
    Disk::Host(HostConn::closed("vps"))
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
    let (panel, _, markdown) = panel(cx);
    let root = PathBuf::from("/srv/app");
    panel.update(cx, |panel, cx| {
        panel.set_root(root.clone(), Disk::Local, cx);
        panel.set_mode(MARKDOWN.id, cx);
    });

    panel.update(cx, |panel, cx| {
        panel.set_root(root.clone(), host_disk(), cx)
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
    let (panel, editor, _) = panel(cx);
    let root = PathBuf::from("/srv/app");
    for disk in [host_disk(), host_disk(), Disk::Local, Disk::Local] {
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
    let (panel, editor, _) = panel(cx);
    let root = PathBuf::from("/srv/app");
    panel.update(cx, |panel, cx| {
        panel.set_root(root.clone(), host_disk(), cx)
    });
    assert!(panel.update(cx, |panel, cx| panel.attach_again(cx)));

    panel.update(cx, |panel, cx| {
        panel.refuse(Some(Refusal::Waiting("Attaching to vps…".into())), cx)
    });
    // Attaching already: nothing to attach again for.
    assert!(!panel.update(cx, |panel, cx| panel.attach_again(cx)));

    editor.borrow_mut().clear();
    panel.update(cx, |panel, cx| {
        panel.set_root(root.clone(), host_disk(), cx)
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
    let (panel, _, _) = panel(cx);
    let taken = Disk::Host(HostConn::taken_from("vps"));
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

/// A switch to a host's root, with a mode on screen not offered there, shows
/// the first mode on the strip that is, and tells it once; a switch that
/// keeps the mode on screen offered tells nobody.
#[gpui::test]
fn a_switch_shows_the_first_mode_offered_when_the_one_showing_is_not(cx: &mut TestAppContext) {
    let (panel, _, _, told) = panel_taking([false, false], cx);
    let root = PathBuf::from("/srv/app");
    panel.update(cx, |panel, cx| {
        panel.set_root(root.clone(), Disk::Local, cx);
        panel.set_mode(MARKDOWN.id, cx);
    });
    told.borrow_mut().clear();
    panel.update(cx, |panel, cx| {
        panel.set_root(root.clone(), host_disk(), cx)
    });
    assert_eq!(*told.borrow(), [(EDITOR.id, "shown")]);
    told.borrow_mut().clear();
    panel.update(cx, |panel, cx| {
        panel.set_root(PathBuf::from("/srv/other"), Disk::Local, cx)
    });
    panel.read_with(cx, |panel, _| assert_eq!(panel.mode(), EDITOR.id));
    assert!(told.borrow().is_empty());
}

/// A file opened goes to the first mode offered that takes it, which shows
/// and is told so; no mode after it is asked.
#[gpui::test]
fn a_file_opens_in_the_first_mode_that_takes_it(cx: &mut TestAppContext) {
    let (panel, _, _, told) = panel_taking([false, true], cx);
    panel.update(cx, |panel, cx| {
        panel.set_root(PathBuf::from("/srv/app"), Disk::Local, cx)
    });
    told.borrow_mut().clear();
    let window = cx.add_empty_window();
    window.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.open_file(Path::new("/srv/app/README.md"), window, cx)
        })
    });
    assert_eq!(
        *told.borrow(),
        [
            (EDITOR.id, "open"),
            (MARKDOWN.id, "open"),
            (MARKDOWN.id, "shown")
        ]
    );
    panel.read_with(window, |panel, _| assert_eq!(panel.mode(), MARKDOWN.id));

    let (panel, _, _, told) = panel_taking([true, true], cx);
    panel.update(cx, |panel, cx| {
        panel.set_root(PathBuf::from("/srv/app"), Disk::Local, cx)
    });
    told.borrow_mut().clear();
    let window = cx.add_empty_window();
    window.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.open_file(Path::new("/srv/app/README.md"), window, cx)
        })
    });
    assert_eq!(*told.borrow(), [(EDITOR.id, "open"), (EDITOR.id, "shown")]);
}

/// While a refusal stands, neither a file opened nor a mode picked changes
/// what shows, and nothing is told.
#[gpui::test]
fn a_refusal_holds_the_mode_showing(cx: &mut TestAppContext) {
    let (panel, _, _, told) = panel_taking([true, true], cx);
    panel.update(cx, |panel, cx| {
        panel.set_root(PathBuf::from("/srv/app"), Disk::Local, cx);
        panel.refuse(Some(Refusal::Waiting("Attaching to vps…".into())), cx);
        panel.set_mode(MARKDOWN.id, cx);
    });
    told.borrow_mut().clear();
    let window = cx.add_empty_window();
    window.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.open_file(Path::new("/srv/app/README.md"), window, cx)
        })
    });
    assert!(told.borrow().is_empty());
    panel.read_with(window, |panel, _| assert_eq!(panel.mode(), EDITOR.id));
}

/// The mode already showing, picked again, is told so again: picking it is
/// how a listing is asked for again.
#[gpui::test]
fn the_mode_showing_picked_again_is_told_again(cx: &mut TestAppContext) {
    let (panel, _, _, told) = panel_taking([false, false], cx);
    panel.update(cx, |panel, cx| {
        panel.set_root(PathBuf::from("/srv/app"), Disk::Local, cx);
        panel.set_mode(MARKDOWN.id, cx);
        panel.set_mode(MARKDOWN.id, cx);
    });
    assert_eq!(
        *told.borrow(),
        [(MARKDOWN.id, "shown"), (MARKDOWN.id, "shown")]
    );
}

/// The modes this binary composes, built as a window builds them, Plugins
/// among them: the Editor alone draws the row in a bar of its own; over every
/// other mode the panel draws it.
#[gpui::test]
fn only_the_editor_draws_the_workbench_row_itself(cx: &mut TestAppContext) {
    use onehand_core::config::{AgentSpec, Preset, drive::SettingsDrive};
    cx.update(gpui_component::init);
    let mut shared = crate::state::Shared::for_test();
    let claude = AgentSpec {
        name: "Claude Code".into(),
        preset: Some(Preset::ClaudeCode.into()),
        ..Default::default()
    };
    shared.settings = SettingsDrive::new(vec![claude], Default::default());
    cx.update(|cx| cx.set_global(shared));
    let panel = cx.update(|cx| {
        Workbench::new(
            |ask, font_size, cx| {
                let tell: onehand_issues::Tell = Rc::new(|_, _, _| {});
                crate::features::workbench_modes(ask, tell, font_size, cx)
            },
            cx,
        )
    });
    let (drawing, others) = panel.read_with(cx, |panel, _| {
        let ids = |own: bool| -> Vec<ModeId> {
            panel
                .modes
                .iter()
                .zip(&panel.own_row)
                .filter(|(_, takes)| **takes == own)
                .map(|(mode, _)| mode.spec().id)
                .collect()
        };
        (ids(true), ids(false))
    });
    assert_eq!(drawing, vec![EDITOR_MODE]);
    assert_eq!(
        others,
        vec![
            MARKDOWN_MODE,
            ISSUES_MODE,
            onehand_claude_plugins::SPEC.id,
            NEOVIM_MODE
        ]
    );
}

/// A bar drawn through the ends' `chrome` inside the body the panel scales
/// lays out at the size it has unzoomed, so the row keeps the header's height
/// in a mode that draws it itself.
#[gpui::test]
fn a_modes_bar_keeps_chrome_size_under_the_panels_zoom(cx: &mut TestAppContext) {
    use std::cell::Cell;
    struct Probe {
        panel: Entity<Workbench>,
        height: Rc<Cell<Pixels>>,
        rem: Rc<Cell<Pixels>>,
    }
    impl Render for Probe {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            self.rem.set(window.rem_size());
            let height = self.height.clone();
            let bar = div().h(gpui::rems(2.)).w_full().child(
                gpui::canvas(
                    move |bounds, _, _| height.set(bounds.size.height),
                    |_, _, _, _| {},
                )
                .size_full(),
            );
            let panel = self.panel.read(cx);
            let bar = (panel.ends.chrome)(bar.into_any_element(), cx);
            div()
                .size_full()
                .child(panel.zoom.scale(div().size_full().child(bar)))
        }
    }
    let (panel, _, _) = panel(cx);
    panel.update(cx, |panel, cx| {
        let mut zoom = panel.zoom();
        zoom.zoom_in();
        zoom.zoom_in();
        panel.set_zoom(zoom, cx);
    });
    let (height, rem) = (
        Rc::new(Cell::new(Pixels::ZERO)),
        Rc::new(Cell::new(Pixels::ZERO)),
    );
    let (h, r) = (height.clone(), rem.clone());
    let (_, cx) = cx.add_window_view(move |_, _| Probe {
        panel,
        height: h,
        rem: r,
    });
    cx.run_until_parked();
    assert!(rem.get() > Pixels::ZERO);
    assert_eq!(height.get(), rem.get() * 2.);
}
