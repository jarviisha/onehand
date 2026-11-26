use super::{MENUS, Toggle, toggle_order};
use crate::keymap::COMMANDS;

/// The toggles at the title bar's end read as the window does, left to
/// right: the terminal under the boxes, the Workbench on the right, and
/// Settings in the corner.
#[test]
fn the_title_bar_end_toggles_terminal_workbench_then_settings() {
    assert_eq!(
        toggle_order(),
        [Toggle::Terminal, Toggle::Workbench, Toggle::Settings]
    );
}

/// A menu item names a command the keymap holds, or the menus cannot be
/// built at all.
#[test]
fn every_menu_item_is_a_command() {
    for (menu, items) in MENUS {
        for (id, _) in items.iter().flatten() {
            assert!(
                COMMANDS.iter().any(|command| command.id == *id),
                "{menu} names `{id}`, which the keymap does not hold"
            );
        }
    }
}

mod strain {
    use crate::shell::Shell;
    use crate::shell::rig::strain::{
        NARROWEST, SHORT, WIDE, assert_inside, assert_one_height, bounds, long, same, unbroken,
        width,
    };
    use crate::shell::rig::{folder, shell};
    use gpui::{Bounds, Entity, Pixels, TestAppContext, VisualTestContext, px};

    /// The widths the window takes: its least, and a wide screen's.
    const WIDTHS: [Pixels; 2] = [NARROWEST, WIDE];

    fn rename(shell: &Entity<Shell>, name: &str, cx: &mut VisualTestContext) {
        cx.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                let fx = shell.window.workspace.rename(name);
                shell.carry_workspace(fx, window, cx);
            })
        });
        cx.run_until_parked();
    }

    /// The parts of the bar a name could push, as drawn.
    #[derive(Debug, PartialEq)]
    struct Bar {
        rail: Bounds<Pixels>,
        toggles: Bounds<Pixels>,
        terminal: Bounds<Pixels>,
        workbench: Bounds<Pixels>,
        settings: Bounds<Pixels>,
    }

    fn bar(cx: &mut VisualTestContext) -> Bar {
        Bar {
            rail: bounds(cx, "title-rail"),
            toggles: bounds(cx, "title-toggles"),
            terminal: bounds(cx, "title-terminal"),
            workbench: bounds(cx, "title-workbench"),
            settings: bounds(cx, "title-settings"),
        }
    }

    /// The names the bar is drawn under: short, two of ordinary length, and
    /// two hundred characters with and without a place to break.
    fn names() -> [String; 5] {
        [
            SHORT.to_string(),
            "Client projects".to_string(),
            "onehand work 2026 Nov".to_string(),
            long(),
            unbroken(),
        ]
    }

    #[gpui::test]
    fn a_long_workspace_name_leaves_the_toggles_in_place(cx: &mut TestAppContext) {
        let dir = folder("strain-title-toggles");
        let (_, shell, cx) = shell(cx, dir.join("tasks"), Vec::new());
        for w in WIDTHS {
            width(cx, w);
            rename(&shell, SHORT, cx);
            let short = bar(cx);
            for name in names() {
                rename(&shell, &name, cx);
                let named = bar(cx);
                assert_eq!(named, short, "at {w:?}, named {} chars", name.len());
                assert!(
                    named.toggles.right() <= w,
                    "at {w:?} the toggles stand past the window: {:?}",
                    named.toggles
                );
            }
        }
    }

    #[gpui::test]
    fn the_palette_stays_at_the_centre_whatever_the_name(cx: &mut TestAppContext) {
        let dir = folder("strain-title-palette");
        let (_, shell, cx) = shell(cx, dir.join("tasks"), Vec::new());
        let rem = cx.update(|window, _| window.rem_size());
        for w in WIDTHS {
            width(cx, w);
            rename(&shell, SHORT, cx);
            let short = bounds(cx, "palette-field");
            for name in names() {
                rename(&shell, &name, cx);
                let field = bounds(cx, "palette-field");
                let at = format!("at {w:?}, named {} chars", name.len());
                assert_eq!(field, short, "{at}: the field moved with the name");
                let (start, toggles) = (bounds(cx, "title-start"), bounds(cx, "title-toggles"));
                assert!(
                    start.right() <= field.left() && field.right() <= toggles.left(),
                    "{at}: {start:?} | {field:?} | {toggles:?} overlap"
                );
                if w == WIDE {
                    assert!(
                        same(
                            field.size.width,
                            crate::tokens::TITLE_SEARCH_W.to_pixels(rem)
                        ),
                        "{at}: the field gave up its width: {field:?}"
                    );
                    // The centre of the bar's content, between the insets the
                    // library's bar keeps at its two ends.
                    let centre = (start.left() + toggles.right()) / 2.;
                    assert!(
                        same(field.center().x, centre),
                        "{at}: the field stands off the bar's centre {centre:?}: {field:?}"
                    );
                }
            }
        }
    }

    #[gpui::test]
    fn a_long_workspace_name_stays_inside_its_slot(cx: &mut TestAppContext) {
        let dir = folder("strain-title-slot");
        let (_, shell, cx) = shell(cx, dir.join("tasks"), Vec::new());
        for w in WIDTHS {
            width(cx, w);
            rename(&shell, SHORT, cx);
            let short = bounds(cx, "title-workspace");
            for name in names() {
                rename(&shell, &name, cx);
                let named = bounds(cx, "title-workspace");
                assert_inside(named, bounds(cx, "title-start"), "the workspace's name");
                assert_one_height(&[named, short], "the workspace's name, long and short");
            }
        }
    }

    #[gpui::test]
    fn a_name_the_narrowest_window_cuts_is_said_whole_in_its_tooltip(cx: &mut TestAppContext) {
        let dir = folder("strain-title-tooltip");
        let (_, shell, cx) = shell(cx, dir.join("tasks"), Vec::new());
        let name = "Client infrastructure";
        rename(&shell, name, cx);
        width(cx, WIDE);
        let whole = bounds(cx, "title-workspace");
        width(cx, NARROWEST);
        let cut = bounds(cx, "title-workspace");
        assert!(
            cut.size.width < whole.size.width,
            "a {}-character name is not cut in the narrowest window: {cut:?} against {whole:?}",
            name.len()
        );
        assert!(
            crate::rail::workspace_hint(name).starts_with(name),
            "the tooltip does not say the name whole first"
        );
    }

    #[gpui::test]
    fn the_title_bar_controls_share_one_height(cx: &mut TestAppContext) {
        let dir = folder("strain-title-height");
        let (_, shell, cx) = shell(cx, dir.join("tasks"), Vec::new());
        for w in WIDTHS {
            width(cx, w);
            for name in [SHORT.to_string(), long()] {
                rename(&shell, &name, cx);
                let bar = bar(cx);
                assert_one_height(
                    &[
                        bar.rail,
                        bar.terminal,
                        bar.workbench,
                        bar.settings,
                        bounds(cx, "title-workspace"),
                    ],
                    "the title bar's controls",
                );
            }
        }
    }

    #[gpui::test]
    fn the_palettes_list_opens_below_its_field(cx: &mut TestAppContext) {
        let dir = folder("strain-title-list");
        let (_, shell, cx) = shell(cx, dir.join("tasks"), Vec::new());
        for w in WIDTHS {
            width(cx, w);
            cx.update(|window, cx| shell.update(cx, |shell, cx| shell.open_palette(window, cx)));
            cx.run_until_parked();
            let (field, list) = (bounds(cx, "palette-field"), bounds(cx, "palette-list"));
            assert!(
                list.top() + px(0.5) >= field.top() + field.size.height
                    || list.bottom() <= field.top(),
                "at {w:?} the list {list:?} covers its field {field:?}"
            );
        }
    }
}
