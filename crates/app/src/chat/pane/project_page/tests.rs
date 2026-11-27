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

/// Each zoom the page is drawn at, its titles short, medium, long and
/// unbroken, on a page of its own: `check` is given the title and its page.
fn each_page(name: &str, seed: u64, check: impl Fn(&str, &str, &mut VisualTestContext)) {
    let mut seed = seed;
    for &(zoom, steps, _) in DRAWN {
        for title in [SHORT.to_string(), medium(), long(), unbroken()] {
            seed += 1;
            let mut app = TestAppContext::single();
            let (shell, cx) = page(
                &format!("{name}-{seed}"),
                seed,
                std::slice::from_ref(&title),
                &mut app,
            );
            zoom_pane(&shell, steps, cx);
            check(zoom, &title, cx);
            app.quit();
        }
    }
}

#[gpui::test]
fn a_rows_facts_give_way_before_its_title(_: &mut TestAppContext) {
    each_page("strain-home-give", 913_060, |zoom, title, cx| {
        width(cx, WIDE);
        let whole = bounds(cx, "home-title-0").size.width;
        if title == SHORT {
            assert!(
                cx.debug_bounds("home-facts-0").is_some(),
                "a short title's facts are not drawn in a wide pane at {zoom}%"
            );
        }
        width(cx, NARROWEST);
        let (row, named, more) = (
            bounds(cx, "home-row-0"),
            bounds(cx, "home-title-0"),
            bounds(cx, "home-more-0"),
        );
        let cut = named.size.width + SLACK < whole;
        let facts = cx.debug_bounds("home-facts-0");
        assert!(
            !(cut && facts.is_some()),
            "at {zoom}% in the narrowest pane the title is cut to {named:?} from {whole:?} while its facts stand at {facts:?}"
        );
        if let Some(facts) = facts {
            assert_inside(facts, row, "a row's facts");
            assert!(
                facts.left() >= named.right(),
                "the facts run over the title"
            );
        }
        assert!(
            named.right() <= more.left() + SLACK,
            "the title runs under the row's ⋯ at {zoom}%"
        );
        if title == SHORT {
            assert!(
                !cut,
                "a short title is cut at {zoom}% in the narrowest pane: {named:?} from {whole:?}"
            );
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
            cx.debug_bounds("page-agent-default").is_some(),
            "the agent's default line is not drawn in a wide pane at {zoom}%"
        );
        width(cx, NARROWEST);
        let (card, column, send) = (
            bounds(cx, "page-card"),
            bounds(cx, "project-page-column"),
            bounds(cx, "page-send"),
        );
        // The agent's default line is dropped once it cannot be said whole.
        let parts = std::iter::once(("page-agent", Some(bounds(cx, "page-agent"))))
            .chain([("page-agent-default", cx.debug_bounds("page-agent-default"))]);
        for (part, at) in parts {
            let Some(at) = at else { continue };
            assert_inside(at, card, &format!("{part} at {zoom}%"));
            assert!(
                at.right() <= send.left() + SLACK,
                "{part} runs under Send at {zoom}%: {at:?} {send:?}"
            );
        }
        for part in ["page-start-in", "page-cards"] {
            assert_inside(bounds(cx, part), column, &format!("{part} at {zoom}%"));
        }
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

/// A title `n` characters long on a page of its own, having continued one
/// titled `earlier`, drawn wide: the title's width, then the bounds of what it
/// continued, of its facts' count and of the rest of its facts, when drawn.
fn continued_row(n: usize, earlier: &str) -> (Pixels, [Option<Bounds<Pixels>>; 3]) {
    let mut app = TestAppContext::single();
    let (_, cx) = page_with(
        &format!("strain-home-continued-{n}-{}", earlier.len()),
        914_000 + n as u64 * 1000 + earlier.len() as u64,
        &["a".repeat(n)],
        Some(earlier),
        &mut app,
    );
    width(cx, WIDE);
    let got = (
        bounds(cx, "home-title-0").size.width,
        [
            cx.debug_bounds("home-continued-0"),
            cx.debug_bounds("home-facts-0"),
            cx.debug_bounds("home-facts-rest-0"),
        ],
    );
    app.quit();
    got
}

#[gpui::test]
fn what_a_row_continued_gives_way_before_its_facts_and_its_title(_: &mut TestAppContext) {
    let earlier = "word ".repeat(6);
    // Short enough that all of it stands whole: every character's advance,
    // and the facts at their whole width.
    let (short, [continued, _, rest]) = continued_row(10, &earlier);
    assert!(
        continued.is_some(),
        "what a short row continued is not drawn"
    );
    let advance = short / 10.;
    let rest_whole = rest.expect("a short row's facts are drawn").size.width;
    let size = |at: Option<Bounds<Pixels>>| at.map_or(Pixels::ZERO, |at| at.size.width);
    for n in (30..82).step_by(3) {
        let (title, [continued, count, rest]) = continued_row(n, &earlier);
        // Any shortfall at all is a character cut, so no slack here.
        let natural = advance * n as f32;
        let cut = title + px(0.01) < natural;
        assert!(
            !(cut && (continued.is_some() || count.is_some())),
            "a {n}-character title is cut to {title:?} of {natural:?} while what it continued ({continued:?}) or its facts ({count:?}) stand"
        );
        if continued.is_some() {
            assert!(
                same(size(rest), rest_whole),
                "what a {n}-character title continued stands while its facts are cut to {:?} of {rest_whole:?}",
                size(rest)
            );
        }
    }
}
