//! A project's page under strain: its past conversations none, one and past
//! the cap, titled long, in the narrowest pane and zoomed, in the box that
//! scrolls them.

use super::HOME_ROWS;
use crate::shell::rig::strain::{
    NARROWEST, SHORT, SLACK, TO_150, WIDE, assert_inside, assert_one_height, bounds, long, same,
    selector, unbroken, width,
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
    page_with(name, seed, titles, None, cx)
}

/// [`page`], each conversation having continued the one titled `continued`
/// when it is given.
fn page_with<'a>(
    name: &str,
    seed: u64,
    titles: &[String],
    continued: Option<&str>,
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
        if let Some(earlier) = continued {
            let at = onehand_core::chat::conversations_dir()
                .join(&sid)
                .join("meta.json");
            let mut meta: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(&at).unwrap()).unwrap();
            meta["prefs"]["continued"] = earlier.into();
            std::fs::write(&at, meta.to_string()).unwrap();
        }
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

/// A title of a few words: longer than a short one, shorter than a wide row.
fn medium() -> String {
    "word ".repeat(8).trim_end().to_string()
}

/// The widths a row is drawn at, narrowest last.
const ROW_WIDTHS: [Pixels; 4] = [WIDE, px(1100.), px(900.), NARROWEST];

/// Each zoom the page is drawn at, its titles short, medium, long and
/// unbroken, each with and without what it continued, on a page of its own:
/// `check` is given the zoom's factor and its page.
fn each_page(name: &str, seed: u64, check: impl Fn(&str, f32, &mut VisualTestContext)) {
    let mut seed = seed;
    for &(zoom, steps, _) in DRAWN {
        let factor = zoom.parse::<f32>().unwrap() / 100.;
        for title in [SHORT.to_string(), medium(), long(), unbroken()] {
            for continued in [None, Some(medium())] {
                seed += 1;
                let mut app = TestAppContext::single();
                let (shell, cx) = page_with(
                    &format!("{name}-{seed}"),
                    seed,
                    std::slice::from_ref(&title),
                    continued.as_deref(),
                    &mut app,
                );
                zoom_pane(&shell, steps, cx);
                check(zoom, factor, cx);
                app.quit();
            }
        }
    }
}

/// Whether a row's facts at `facts` show in its middle at `middle`: on its
/// first line, not on the one out of sight.
fn shown(facts: Bounds<Pixels>, middle: Bounds<Pixels>) -> bool {
    facts.top() + SLACK < middle.bottom()
}

#[gpui::test]
fn a_rows_facts_stand_whole_or_not_at_all(_: &mut TestAppContext) {
    each_page("strain-home-facts", 913_100, |zoom, factor, cx| {
        let (least, line) = cx.update(|window, _| {
            let rem = window.rem_size() * factor;
            (
                super::TITLE_LEAST.to_pixels(rem),
                onehand_ui::tokens::ROW_H.to_pixels(rem),
            )
        });
        width(cx, WIDE);
        let whole = bounds(cx, "home-facts-0").size.width;
        assert!(
            shown(bounds(cx, "home-facts-0"), bounds(cx, "home-middle-0")),
            "a row's facts are not shown in a wide pane at {zoom}%"
        );
        for w in ROW_WIDTHS {
            width(cx, w);
            let (row, middle, named, facts, more) = (
                bounds(cx, "home-row-0"),
                bounds(cx, "home-middle-0"),
                bounds(cx, "home-title-0"),
                bounds(cx, "home-facts-0"),
                bounds(cx, "home-more-0"),
            );
            assert_inside(more, row, &format!("the row's ⋯ at {w:?}, {zoom}%"));
            // One line high, whatever it wrapped out of sight.
            assert!(
                same(row.size.height, line),
                "a row stands {:?} high, not one line's {line:?}, at {w:?}, {zoom}%",
                row.size.height
            );
            assert!(
                named.size.width + SLACK >= least.min(middle.size.width),
                "the title is down to {named:?} under its least {least:?} at {w:?}, {zoom}%"
            );
            if shown(facts, middle) {
                assert_inside(facts, middle, &format!("a row's facts at {w:?}, {zoom}%"));
                assert!(
                    same(facts.size.width, whole),
                    "a row's facts are drawn at {facts:?}, narrower than their words' {whole:?}, at {w:?}, {zoom}%"
                );
                assert!(
                    same(facts.right(), middle.right()),
                    "a row's facts leave its end at {w:?}, {zoom}%"
                );
                assert!(
                    facts.left() >= named.right(),
                    "the facts run over the title"
                );
                assert!(
                    (facts.center().y - named.center().y).abs() <= px(1.),
                    "a row's facts stand off its title's line at {w:?}, {zoom}%"
                );
            } else {
                assert!(
                    facts.top() + SLACK >= middle.bottom(),
                    "a row's facts are partly in sight at {w:?}, {zoom}%: {facts:?} in {middle:?}"
                );
            }
            if w == NARROWEST {
                assert!(
                    !shown(facts, middle),
                    "a row's facts are shown in the narrowest pane at {zoom}%"
                );
            }
        }
    });
}

#[gpui::test]
fn the_heads_place_stays_on_one_line_beside_its_actions(_: &mut TestAppContext) {
    for (seed, &(zoom, steps, _)) in (913_080..).zip(DRAWN) {
        let mut app = TestAppContext::single();
        let (shell, cx) = page(&format!("strain-home-head-{zoom}"), seed, &[], &mut app);
        zoom_pane(&shell, steps, cx);
        width(cx, WIDE);
        let line = bounds(cx, "project-page-path").size.height;
        width(cx, NARROWEST);
        let (path, head, actions) = (
            bounds(cx, "project-page-path"),
            bounds(cx, "page-head"),
            bounds(cx, "page-head-actions"),
        );
        assert!(
            same(path.size.height, line),
            "the path breaks onto a second line at {zoom}%: {path:?}"
        );
        assert_inside(path, head, "the head's path");
        assert!(
            path.right() <= actions.left() + SLACK,
            "the path runs under the head's actions at {zoom}%: {path:?} {actions:?}"
        );
        app.quit();
    }
}

#[gpui::test]
fn the_composers_controls_stay_inside_their_boxes(_: &mut TestAppContext) {
    for (seed, &(zoom, steps, _)) in (913_090..).zip(DRAWN) {
        let mut app = TestAppContext::single();
        let (shell, cx) = page(&format!("strain-home-composer-{zoom}"), seed, &[], &mut app);
        zoom_pane(&shell, steps, cx);
        width(cx, NARROWEST);
        width(cx, WIDE);
        assert!(
            bounds(cx, "page-agent-default").top() + SLACK < bounds(cx, "page-agent-row").bottom(),
            "the agent's default line is not shown in a wide pane at {zoom}%"
        );
        width(cx, NARROWEST);
        let (card, column, send) = (
            bounds(cx, "page-card"),
            bounds(cx, "project-page-column"),
            bounds(cx, "page-send"),
        );
        // The agent's default line goes whole, out of sight, once it does
        // not fit beside the agent's name, and the row keeps its height.
        let line = bounds(cx, "page-agent-row");
        let agent = bounds(cx, "page-agent");
        assert!(
            same(line.size.height, agent.size.height),
            "the agent's row grows to {line:?} beside its picker's {agent:?} at {zoom}%"
        );
        let parts = std::iter::once(("page-agent", bounds(cx, "page-agent")))
            .chain([("page-agent-default", bounds(cx, "page-agent-default"))]);
        for (part, at) in parts {
            if at.top() + SLACK >= line.bottom() {
                continue;
            }
            assert!(
                at.right() <= line.right() + SLACK,
                "{part} is cut at {zoom}%: {at:?} in {line:?}"
            );
            assert_inside(at, card, &format!("{part} at {zoom}%"));
            assert!(
                at.right() <= send.left() + SLACK,
                "{part} runs under Send at {zoom}%: {at:?} {send:?}"
            );
        }
        for part in ["page-start-in", "page-cards"] {
            assert_inside(bounds(cx, part), column, &format!("{part} at {zoom}%"));
        }
        // With no branch field the strip keeps one line.
        assert!(
            same(
                bounds(cx, "page-start-in").top(),
                bounds(cx, "page-cards").top()
            ),
            "the strip breaks onto a second line at {zoom}%"
        );
        // A new worktree's branch field beside them, or under them when the
        // strip is short of room: it keeps its width, or the column's
        // whole, and squeezes neither chip.
        let pane = cx.update(|_, cx| shell.read(cx).chat_pane().clone());
        cx.update(|window, cx| {
            pane.update(cx, |pane, cx| {
                pane.set_page_start(crate::chat::pane::StartIn::NewWorktree, None, window, cx);
            })
        });
        cx.run_until_parked();
        width(cx, WIDE);
        let wide = ["page-branch", "page-start-in", "page-cards"].map(|part| bounds(cx, part));
        width(cx, NARROWEST);
        let column = bounds(cx, "project-page-column");
        let field = bounds(cx, "page-branch");
        assert_inside(field, column, &format!("the branch field at {zoom}%"));
        assert!(
            field.size.width + SLACK >= wide[0].size.width.min(column.size.width),
            "the branch field is down to {field:?} at {zoom}%"
        );
        let cards = bounds(cx, "page-cards");
        assert!(
            same(cards.right(), column.right()),
            "the Cards chip leaves the strip's end at {zoom}%: {cards:?} in {column:?}"
        );
        for (part, wide) in ["page-start-in", "page-cards"].into_iter().zip(&wide[1..]) {
            let at = bounds(cx, part);
            assert_inside(
                at,
                column,
                &format!("{part} beside the branch field at {zoom}%"),
            );
            assert!(
                same(at.size.width, wide.size.width),
                "{part} is squeezed to {at:?} beside the branch field at {zoom}%"
            );
        }
        app.quit();
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
