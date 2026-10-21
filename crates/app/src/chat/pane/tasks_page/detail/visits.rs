//! A run's visit timeline: each visit's row, what it ran with, and, once
//! opened, what it kept, printed and changed.

use super::{
    ChatPane, Dirs, FOLD_INDENT, TaskDetail, fold_row, mono_well, muted_line, rel_time, slot,
};
use gpui::{
    App, Context, InteractiveElement, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement, Styled, div,
};
use gpui_component::{ActiveTheme, StyledExt};
use onehand_core::chat::now_secs;
use onehand_core::workflow::{Run, Settings, Visit};

/// How many visits a run's timeline draws, its newest ones.
const VISITS_SHOWN: usize = 100;

/// How many changed files a visit lists.
const FILES_SHOWN: usize = 100;

/// How many lines of one file's diff are drawn.
const DIFF_LINES: usize = 400;

/// A fold row's chevron, at the dense icon size.
const CHEVRON_W: gpui::Rems = gpui::rems(0.75);

/// `run`'s visits, oldest first, each opening onto what it kept and what
/// it changed. `live` when the run is at work, so its open visit is under
/// way rather than cut off.
pub(super) fn timeline(
    detail: &TaskDetail,
    run: &Run,
    live: bool,
    cx: &mut Context<ChatPane>,
) -> Vec<gpui::AnyElement> {
    let muted = cx.theme().muted_foreground;
    let visits = run.visits();
    if visits.is_empty() {
        return vec![muted_line("It has not started", cx)];
    }
    let hidden = visits.len().saturating_sub(VISITS_SHOWN);
    let now = now_secs();
    let mut out: Vec<gpui::AnyElement> = Vec::new();
    if hidden > 0 {
        out.push(muted_line(
            &format!("{hidden} earlier visits not shown"),
            cx,
        ));
    }
    for visit in &visits[hidden..] {
        let key = (run.id.clone(), visit.id);
        let open = detail.expanded.contains(&key);
        let label = run
            .template
            .steps
            .iter()
            .find(|step| step.id == visit.step)
            .map_or_else(|| visit.step.clone(), |step| step.label.clone());
        let started = rel_time(now, visit.started_at);
        // A cut-off visit has no end to measure to, and its time would grow
        // for ever.
        let until = match (visit.ended_at, live) {
            (Some(at), _) => Some(at),
            (None, true) => Some(now),
            (None, false) => None,
        };
        let when = match until {
            Some(at) => format!(
                "{started} · {}",
                onehand_core::duration(at.saturating_sub(visit.started_at))
            ),
            None => started,
        };
        let why = visit.why.clone().unwrap_or_else(|| {
            match live {
                true => "In progress",
                false => "Cut off",
            }
            .to_string()
        });
        let pinned = visit.start.clone().zip(visit.end.clone());
        let dirs = dirs(run);
        out.push(
            fold_row(
                SharedString::from(format!("task-visit-{}-{}", run.id, visit.id)),
                open,
                cx,
            )
            .on_click(cx.listener(move |pane: &mut ChatPane, _, _, cx| {
                pane.toggle_visit(key.clone(), pinned.clone(), dirs.clone(), cx)
            }))
            .child(div().flex_none().child(label))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_xs()
                    .text_color(muted)
                    .child(why),
            )
            .child(slot(when, cx))
            .into_any_element(),
        );
        let said = visit
            .ran_with
            .as_ref()
            .map(Settings::said)
            .unwrap_or_default();
        if !said.is_empty() {
            out.push(
                div()
                    .pl(FOLD_INDENT)
                    .text_xs()
                    .text_color(muted)
                    .child(said)
                    .into_any_element(),
            );
        }
        if open {
            out.push(
                div()
                    .v_flex()
                    .gap_2()
                    .pl(FOLD_INDENT)
                    .children(visit_body(detail, run, visit, live, cx))
                    .into_any_element(),
            );
        }
    }
    out
}

/// An opened visit: what it kept or printed, then the files it changed.
fn visit_body(
    detail: &TaskDetail,
    run: &Run,
    visit: &Visit,
    live: bool,
    cx: &mut Context<ChatPane>,
) -> Vec<gpui::AnyElement> {
    let mut out = Vec::new();
    if let Some(ran) = &visit.command {
        out.push(muted_line(&format!("The command {}", ran.said()), cx));
    }
    // A failed command's output is the visit's own; a passed one's only its
    // result kept.
    let printed = visit.output.as_deref().or_else(|| {
        visit
            .command
            .as_ref()
            .filter(|ran| ran.passed)
            .map(|ran| ran.tail.as_str())
    });
    if let Some(printed) = printed.filter(|text| !text.trim().is_empty()) {
        out.extend(mono_well(printed, cx));
    }
    let (Some(from), Some(to)) = (visit.start.clone(), visit.end.clone()) else {
        let said = match (visit.ended_at, live) {
            (None, true) => "In progress",
            (None, false) => "Cut off before its end was pinned",
            (Some(_), _) => "No marks were pinned for this visit",
        };
        out.push(muted_line(said, cx));
        return out;
    };
    let changes = match detail.changes.get(&(from.clone(), to.clone())) {
        Some(Some(Ok(changes))) => changes,
        Some(Some(Err(why))) => {
            out.push(warning_line(
                &format!("Reading what changed failed: {why}"),
                cx,
            ));
            return out;
        }
        Some(None) | None => {
            out.push(muted_line("Reading what changed…", cx));
            return out;
        }
    };
    if changes.is_empty() {
        out.push(muted_line("No files changed", cx));
    }
    let muted = cx.theme().muted_foreground;
    for (i, change) in changes.iter().take(FILES_SHOWN).enumerate() {
        let key = (from.clone(), to.clone(), change.path.clone());
        let opened = detail.files.get(&key);
        let counts = match change.lines {
            Some((added, removed)) => {
                crate::chat::transcript::line_counts(added as usize, removed as usize, cx)
            }
            None => Some(div().text_color(muted).child("binary")),
        };
        let dirs = dirs(run);
        let id = SharedString::from(format!("task-file-{}-{}-{i}", run.id, visit.id));
        // A binary file has no lines to diff: it is listed, never opened, its
        // chevron's slot kept so its path starts where the others do.
        let line = match change.lines {
            Some(_) => fold_row(id, opened.is_some(), cx).on_click(cx.listener(
                move |pane: &mut ChatPane, _, _, cx| {
                    pane.toggle_file(key.clone(), dirs.clone(), cx)
                },
            )),
            None => onehand_ui::row(false, cx)
                .id(id)
                .h_flex()
                .items_center()
                .gap_2()
                .child(div().flex_none().w(CHEVRON_W)),
        };
        out.push(
            line.child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .font_family(cx.theme().mono_font_family.clone())
                    .text_size(crate::tokens::CODE_TEXT)
                    .child(change.path.clone()),
            )
            .children(counts.map(|counts| counts.text_xs()))
            .into_any_element(),
        );
        match opened {
            Some(Some(Ok(rows))) => {
                let mut budget = DIFF_LINES;
                let drawn = crate::chat::transcript::diff_rows(rows, &mut budget, cx);
                let cut = rows.len().saturating_sub(DIFF_LINES);
                out.push(
                    onehand_ui::well(cx)
                        .v_flex()
                        .children(drawn)
                        .into_any_element(),
                );
                if cut > 0 {
                    out.push(muted_line(&format!("{cut} more diff lines not shown"), cx));
                }
            }
            Some(Some(Err(why))) => {
                out.push(warning_line(&format!("Reading the diff failed: {why}"), cx))
            }
            Some(None) => out.push(muted_line("Reading the diff…", cx)),
            None => {}
        }
    }
    let hidden = changes.len().saturating_sub(FILES_SHOWN);
    if hidden > 0 {
        out.push(muted_line(&format!("{hidden} more files not shown"), cx));
    }
    out
}

/// A read that failed, said in the warning ink.
fn warning_line(text: &str, cx: &App) -> gpui::AnyElement {
    div()
        .text_xs()
        .text_color(crate::theme::status_ink(cx).warning)
        .child(text.to_string())
        .into_any_element()
}

/// Where `run`'s marks are read.
fn dirs(run: &Run) -> Dirs {
    (run.setup.dir.clone(), run.setup.repo.clone())
}
