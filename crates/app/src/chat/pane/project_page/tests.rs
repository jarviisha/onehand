//! A project's page under strain: its past conversations none, one and past
//! the cap, titled long, in the narrowest pane and zoomed, in the box that
//! scrolls them.

use super::HOME_ROWS;
use crate::shell::rig::strain::{
    NARROWEST, SHORT, TO_150, WIDE, assert_inside, assert_one_height, bounds, long, same, selector,
    unbroken, width,
};
use crate::shell::rig::{folder, keep_projects, kept_conversation, nowhere, shell_on, zoom_pane};
use gpui::{
    Bounds, Entity, Modifiers, Pixels, ScrollDelta, ScrollWheelEvent, TestAppContext, TouchPhase,
    VisualTestContext, point, px, size,
};
use onehand_core::config::PanelLayout;
use onehand_core::project::{Project, ProjectId};
use onehand_core::workspace::Workspace;
use std::time::Duration;

/// A project page of a project holding `titles`' conversations, beside the
/// narrowest rail, drawn once they are read.
fn page<'a>(
    name: &str,
    seed: u64,
    titles: &[String],
    cx: &'a mut TestAppContext,
) -> (Entity<crate::shell::Shell>, &'a mut VisualTestContext) {
    let dir = folder(name);
    let home = dir.join("p");
    std::fs::create_dir_all(&home).unwrap();
    let home = std::fs::canonicalize(home).unwrap();
    let mut ws = Workspace::seeded("/tmp");
    ws.layout.rail_w = PanelLayout::RAIL_MIN;
    let (handle, shell, cx) = shell_on(cx, dir.join("tasks"), vec![nowhere()], ws);
    let project = Project::new(ProjectId::mint(seed), None, home.clone());
    keep_projects(handle, &shell, vec![project.clone()], cx);
    for (i, title) in titles.iter().enumerate() {
        let sid = format!("{name}-{i}-{}", std::process::id());
        kept_conversation(&sid, &home, &project.id, Some(title));
    }
    let at = home.clone();
    cx.update(|window, cx| shell.update(cx, |shell, cx| shell.open_root(at, window, cx)));
    let idx = cx.update(|_, cx| {
        shell
            .read(cx)
            .workspace_window()
            .workspace
            .roots
            .iter()
            .position(|root| root.path == home)
            .expect("the project's row")
    });
    cx.update(|window, cx| shell.update(cx, |shell, cx| shell.show_project_page(idx, window, cx)));
    let want = titles.len().min(HOME_ROWS);
    for _ in 0..300 {
        cx.run_until_parked();
        let drawn = (0..want)
            .filter(|i| cx.debug_bounds(selector(format!("home-row-{i}"))).is_some())
            .count();
        if drawn == want && cx.debug_bounds("project-page").is_some() {
            return (shell, cx);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("the page never drew its {want} rows");
}

fn rows(n: usize, cx: &mut VisualTestContext) -> Vec<(Bounds<Pixels>, Bounds<Pixels>)> {
    (0..n)
        .map(|i| {
            (
                bounds(cx, selector(format!("home-row-{i}"))),
                bounds(cx, selector(format!("home-more-{i}"))),
            )
        })
        .collect()
}

fn titled(n: usize, title: &str) -> Vec<String> {
    vec![title.to_string(); n]
}

#[gpui::test]
fn a_project_page_with_no_conversations_draws_no_rows(cx: &mut TestAppContext) {
    let (_, cx) = page("strain-home-none", 913_001, &[], cx);
    assert!(cx.debug_bounds("home-row-0").is_none());
    assert!(cx.debug_bounds("home-hidden").is_none());
}

#[gpui::test]
fn past_the_cap_the_page_says_how_many_more(cx: &mut TestAppContext) {
    let (_, cx) = page(
        "strain-home-cap",
        913_002,
        &titled(HOME_ROWS + 1, SHORT),
        cx,
    );
    assert!(
        cx.debug_bounds(selector(format!("home-row-{}", HOME_ROWS - 1)))
            .is_some()
    );
    assert!(
        cx.debug_bounds(selector(format!("home-row-{HOME_ROWS}")))
            .is_none()
    );
    bounds(cx, "home-hidden");
}

/// The zooms and the widths the rows are drawn at.
const DRAWN: &[(&str, usize, &[Pixels])] = &[
    ("100", 0, &[NARROWEST, WIDE]),
    ("150", TO_150, &[NARROWEST, WIDE]),
];

fn rows_hold(
    name: &str,
    seed: u64,
    n: usize,
    drawn: &[(&str, usize, &[Pixels])],
    cx: &mut TestAppContext,
) {
    let shown = n.min(HOME_ROWS);
    for &(zoom, steps, widths) in drawn {
        for title in [long(), unbroken()] {
            // A fresh page of short titles and one of long, at the same zoom.
            let mut short_cx = TestAppContext::single();
            let (short_shell, short) = page(
                &format!("{name}-s{zoom}"),
                seed,
                &titled(n, SHORT),
                &mut short_cx,
            );
            zoom_pane(&short_shell, steps, short);
            let (shell, cx) = page(&format!("{name}-l{zoom}"), seed + 1, &titled(n, &title), cx);
            zoom_pane(&shell, steps, cx);
            for &w in widths {
                width(short, w);
                width(cx, w);
                let want = rows(shown, short);
                let got = rows(shown, cx);
                let page = bounds(cx, "project-page");
                for (i, ((row, more), (short_row, short_more))) in got.iter().zip(&want).enumerate()
                {
                    assert!(
                        same(row.size.height, short_row.size.height),
                        "row {i} at {w:?}, {zoom}%: {row:?} against a short title's {short_row:?}"
                    );
                    assert!(
                        same(more.size.width, short_more.size.width),
                        "row {i}'s trailing slot at {w:?}, {zoom}% gave way to its title"
                    );
                    assert_inside(
                        *more,
                        *row,
                        &format!(
                            "a row's trailing slot at {w:?}, {zoom}% (a short title's: {short_more:?} in {short_row:?})"
                        ),
                    );
                    assert_eq!(
                        row.left().max(page.left()),
                        row.left(),
                        "row {i} starts outside the page"
                    );
                    assert!(
                        row.right() <= page.right(),
                        "row {i} runs past the page: {row:?} {page:?}"
                    );
                }
                assert_one_height(
                    &got.iter().map(|(row, _)| *row).collect::<Vec<_>>(),
                    "the rows",
                );
            }
            short_cx.quit();
        }
    }
}

#[gpui::test]
fn one_history_row_keeps_its_height_and_its_trailing_slot(cx: &mut TestAppContext) {
    rows_hold("strain-home-one", 913_010, 1, DRAWN, cx);
}

#[gpui::test]
fn every_history_row_keeps_its_height_and_its_trailing_slot(cx: &mut TestAppContext) {
    rows_hold("strain-home-full", 913_020, HOME_ROWS + 1, DRAWN, cx);
}

/// Scroll the page's box to its end, from a point inside it.
fn scroll_to_end(cx: &mut VisualTestContext) {
    let page = bounds(cx, "project-page");
    cx.simulate_event(ScrollWheelEvent {
        position: page.center(),
        delta: ScrollDelta::Pixels(point(px(0.), px(-100_000.))),
        modifiers: Modifiers::none(),
        touch_phase: TouchPhase::Moved,
    });
    cx.run_until_parked();
}

/// A page taller than its window: past the cap, titled long, in a short window.
fn overfull<'a>(name: &str, seed: u64, cx: &'a mut TestAppContext) -> &'a mut VisualTestContext {
    let (_, cx) = page(name, seed, &titled(HOME_ROWS + 1, &long()), cx);
    cx.simulate_resize(size(NARROWEST, crate::shell::rig::strain::SHORT_WINDOW_H));
    cx.run_until_parked();
    let (page, column) = (
        bounds(cx, "project-page"),
        bounds(cx, "project-page-column"),
    );
    assert!(
        column.bottom() > page.bottom(),
        "the page fits its window, so nothing here scrolls: {column:?} in {page:?}"
    );
    cx
}

#[gpui::test]
fn the_last_row_stands_off_the_end_of_its_box(cx: &mut TestAppContext) {
    let cx = overfull("strain-home-end", 913_030, cx);
    let (page, column) = (
        bounds(cx, "project-page"),
        bounds(cx, "project-page-column"),
    );
    let top = column.top() - page.top();
    scroll_to_end(cx);
    let (page, column) = (
        bounds(cx, "project-page"),
        bounds(cx, "project-page-column"),
    );
    let bottom = page.bottom() - column.bottom();
    assert!(
        bottom > Pixels::ZERO && same(bottom, top),
        "scrolled to its end, the page's last part stands {bottom:?} off the box's end, against {top:?} at its top"
    );
}

#[gpui::test]
fn rows_in_a_scrolling_box_keep_a_lane_beside_its_scrollbar(cx: &mut TestAppContext) {
    let cx = overfull("strain-home-lane", 913_040, cx);
    let page = bounds(cx, "project-page");
    for (i, (row, _)) in rows(HOME_ROWS, cx).into_iter().enumerate() {
        assert!(
            row.right() + onehand_ui::tokens::SCROLLBAR_W <= page.right(),
            "row {i} runs under the scrollbar: {row:?} in {page:?}"
        );
    }
}
