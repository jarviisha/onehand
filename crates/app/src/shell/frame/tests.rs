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
    use crate::shell::boot::open::WINDOW_MIN;
    use crate::shell::rig::strain::{
        SHORT, assert_inside, assert_one_height, bounds, long, same, unbroken, width,
    };
    use crate::shell::rig::{folder, shell};
    use gpui::{Bounds, Entity, Pixels, TestAppContext, VisualTestContext, px};

    /// A wide screen's width.
    const WIDE: Pixels = px(1920.);

    /// The widths the window takes: its least, and a wide screen's.
    const WIDTHS: [Pixels; 2] = [WINDOW_MIN.width, WIDE];

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

    fn names() -> [String; 2] {
        [long(), unbroken()]
    }

    fn toggles_stay_put(widths: &[Pixels], cx: &mut TestAppContext) {
        let dir = folder(&format!("strain-title-toggles-{}", widths.len()));
        let (_, shell, cx) = shell(cx, dir.join("tasks"), Vec::new());
        for &w in widths {
            width(cx, w);
            rename(&shell, SHORT, cx);
            let short = bar(cx);
            for name in names() {
                rename(&shell, &name, cx);
                assert_eq!(bar(cx), short, "at {w:?}, named {} chars", name.len());
            }
        }
    }

    #[gpui::test]
    fn a_long_workspace_name_leaves_the_toggles_in_place(cx: &mut TestAppContext) {
        toggles_stay_put(&[WIDE], cx);
    }

    #[gpui::test]
    #[ignore = "a name of about twenty characters pushes the toggles out of the narrowest window; the fix waits for a decision"]
    fn a_long_workspace_name_leaves_the_toggles_in_place_in_the_narrowest_window(
        cx: &mut TestAppContext,
    ) {
        toggles_stay_put(&[WINDOW_MIN.width, WIDE], cx);
    }

    fn palette_stays_put(widths: &[Pixels], cx: &mut TestAppContext) {
        let dir = folder(&format!("strain-title-palette-{}", widths.len()));
        let (_, shell, cx) = shell(cx, dir.join("tasks"), Vec::new());
        for &w in widths {
            width(cx, w);
            rename(&shell, SHORT, cx);
            let short = bounds(cx, "palette-field");
            for name in [SHORT.to_string(), long(), unbroken()] {
                rename(&shell, &name, cx);
                let field = bounds(cx, "palette-field");
                assert_eq!(field, short, "at {w:?}, the field moved with the name");
                let (start, toggles) = (bounds(cx, "title-start"), bounds(cx, "title-toggles"));
                if w == WIDE {
                    // The centre of the bar's content, inside its own insets.
                    let centre = (start.left() + toggles.right()) / 2.;
                    assert!(
                        same(field.center().x, centre),
                        "the field stands off the bar's centre {centre:?}: {field:?}"
                    );
                }
                assert!(
                    start.right() <= field.left() && field.right() <= toggles.left(),
                    "at {w:?}: {start:?} | {field:?} | {toggles:?} overlap"
                );
            }
        }
    }

    #[gpui::test]
    fn the_palette_stays_at_the_centre_whatever_the_name(cx: &mut TestAppContext) {
        palette_stays_put(&[WIDE], cx);
    }

    #[gpui::test]
    #[ignore = "a name of about fifteen characters moves the palette in the narrowest window; the fix waits for a decision"]
    fn the_palette_stays_put_whatever_the_name_in_the_narrowest_window(cx: &mut TestAppContext) {
        palette_stays_put(&[WINDOW_MIN.width, WIDE], cx);
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
