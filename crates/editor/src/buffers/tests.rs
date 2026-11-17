use super::*;
use gpui::{Context, Render, TestAppContext, px};
use gpui_component::ElementExt as _;
use onehand_workbench::{Ask, Asker, Disk, WorkbenchMode as _};
use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;

/// The dropdown drawn on its own, recording the height it was laid out at.
struct Probe {
    tree: AnyView,
    height: Rc<Cell<gpui::Pixels>>,
}

impl Render for Probe {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let height = self.height.clone();
        let close = Rc::new(|_: &usize, _: &mut Window, _: &mut App| {});
        div().size_full().child(
            tree_dropdown(self.tree.clone(), 0, close, cx)
                .on_prepaint(move |bounds, _, _| height.set(bounds.size.height)),
        )
    }
}

fn folder(name: &str, files: usize) -> PathBuf {
    let root = std::env::temp_dir().join(format!("onehand-dropdown-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    for i in 0..files {
        std::fs::write(root.join(format!("f{i}.rs")), "").unwrap();
    }
    root
}

/// The height the dropdown takes over a project of `files` files.
fn drawn(cx: &mut TestAppContext, name: &str, files: usize) -> (gpui::Pixels, gpui::Pixels) {
    cx.update(gpui_component::init);
    let root = folder(name, files);
    let ask: Asker = Rc::new(|_: &Ask<'_>, _: &mut Window, _: &mut App| {});
    let mut mode = cx.update(|cx| onehand_files::Mode::new(ask, cx));
    cx.update(|cx| mode.set_root(&root, &Disk::Local, cx));
    cx.run_until_parked();
    let height = Rc::new(Cell::new(px(0.)));
    let tree = mode.dropdown_view();
    let (_, cx) = cx.add_window_view({
        let height = height.clone();
        move |_, _| Probe { tree, height }
    });
    cx.run_until_parked();
    for _ in 0..3 {
        cx.update(|window, _| window.refresh());
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }
    let rem = cx.update(|window, _| window.rem_size());
    std::fs::remove_dir_all(&root).unwrap();
    (height.get(), rem)
}

#[gpui::test]
fn a_short_tree_stands_at_its_rows_height(cx: &mut TestAppContext) {
    let (height, rem) = drawn(cx, "short", 3);
    let row = onehand_ui::tokens::TREE_ROW_H.to_pixels(rem);
    // The head's row and the three files'.
    assert!(height >= row * 4., "three rows laid out in {height:?}");
    assert!(height < gpui::rems(24.).to_pixels(rem), "{height:?}");
}

#[gpui::test]
fn a_long_tree_stops_at_the_cap(cx: &mut TestAppContext) {
    let (height, rem) = drawn(cx, "long", 40);
    let cap = gpui::rems(24.).to_pixels(rem);
    // The cap is the tree's; the head stands over it.
    let head = gpui::rems(3.).to_pixels(rem);
    assert!(
        height > cap && height < cap + head,
        "{height:?} under {cap:?}"
    );
}
