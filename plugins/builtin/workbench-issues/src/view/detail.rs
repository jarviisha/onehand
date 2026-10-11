use super::list::{chip, identity};
use super::mentions::FILE_LINK;
use super::work;
use super::{FILES_SHOWN, HISTORY_SHOWN, IssuesView};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, ClickEvent, Context, Entity, HighlightStyle, InteractiveElement as _,
    IntoElement, ParentElement, Rems, SharedString, StatefulInteractiveElement as _, Styled,
    WeakEntity, Window, div, rems,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::text::{TextView, TextViewState, TextViewStyle};
use gpui_component::tooltip::Tooltip;
use gpui_component::{ActiveTheme, Icon, IconName, Sizable as _, StyledExt};
use onehand_core::issues::template::{IssueTemplate, lacking};
use onehand_core::issues::{LocalIssue, sync};
use onehand_core::task::Group;
use onehand_core::task::work::{IssueWork, PrSeen};
use onehand_plugin_host::{AppIcon, BAR_H, Tint, action, status_ink, tint};
use std::path::Path;

/// The widest a description's lines run: past it a line is too long to
/// find the start of the next one.
const BODY_MEASURE: Rems = rems(45.);

/// A description's text: the UI's own size, so an issue reads as part of the
/// window rather than as a document dropped into it.
const BODY_TEXT: Rems = rems(0.875);

/// A description's first heading, at a section title's size, and its second
/// a step under it; the rest are the body's size, told apart by weight.
const HEADING_1: Rems = rems(1.);
const HEADING_2: Rems = rems(0.9375);

/// The gutter inside a fenced block, wider than a row's so the code reads as
/// set in its well rather than on its edge.
const CODE_INSET: Rems = rems(0.75);

/// A fenced block's text, a step under the body's, as machine text is
/// everywhere else.
const CODE_TEXT: Rems = rems(0.8125);

/// What is being done with an issue: the live session its history names,
/// whether a run may be started on its project, and its work as the app told
/// it, with the pull request as last read.
pub(super) struct Doing<'a> {
    pub(super) session: Option<String>,
    pub(super) offered: bool,
    pub(super) work: Option<IssueWork>,
    pub(super) pr: PrSeen<'a>,
    /// The pull request as last read for this work, kept beside a failed
    /// read, and when it was read.
    pub(super) last: Option<&'a super::reads::PrRead>,
    /// On the Issues page, what the full form adds: the steps to come, and
    /// what the work left as read off git.
    pub(super) full: Option<super::full::Full<'a>>,
    /// On the Issues page, the review block, while a person has it open.
    pub(super) review: Option<AnyElement>,
    /// The templates the issue's project offers, which its body is read
    /// against.
    pub(super) templates: Vec<IssueTemplate>,
    /// Its history is open rather than folded.
    pub(super) history_open: bool,
}

/// One issue, read, in one fixed order whatever the state, so what it waits
/// on comes before what it says: who it is, where its work stands, what it
/// asks for, what the work left, and what came before.
pub(super) fn issue_view(
    root: &Path,
    issue: &LocalIssue,
    body: Option<(Entity<TextViewState>, Vec<String>)>,
    publish_to: Option<&'static str>,
    doing: Doing<'_>,
    window: &mut Window,
    cx: &mut Context<IssuesView>,
) -> AnyElement {
    let number = issue.number;
    let open = issue.open;
    let muted = cx.theme().muted_foreground;
    let next = work::next_for(issue, &doing);
    // Who it is, and what to do about it on the same row: the title wraps
    // rather than cutting, since it is the one place the whole of it is
    // read, and the row is never shorter than the bar beside it, so the
    // title stands level with the list's head.
    let header = div()
        .h_flex()
        .items_center()
        .gap_2()
        .w_full()
        .flex_none()
        .min_h(BAR_H)
        .px_4()
        .py_1()
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_base()
                .font_medium()
                .child(issue.title.clone()),
        )
        .child(work::actions(root, issue, &doing, &next, publish_to, cx));

    // Its state as a badge, then where it lives, its labels, how its work
    // stands, when it last changed, and how urgent the body says it is.
    let reference = match (issue.reference(), &issue.link) {
        (Some(reference), Some(link)) => div()
            .id("issue-reference")
            .flex_none()
            .text_color(muted)
            .cursor_pointer()
            .hover(|reference| reference.underline())
            .tooltip({
                let tip = format!("Open on {}", link.connector);
                move |window, cx| Tooltip::new(tip.clone()).build(window, cx)
            })
            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                view.with_url(number, |url, cx| cx.open_url(&url), cx)
            }))
            .child(reference.to_string())
            .into_any_element(),
        _ => identity(issue, cx),
    };
    let work = doing.work.as_ref().map(|work| &work.work);
    let run = match work {
        Some(work) => {
            let ink = if work.group.needs_attention() {
                status_ink(cx).warning
            } else if work.group == Group::Running {
                cx.theme().link
            } else {
                muted
            };
            div().flex_none().text_color(ink).child(work.said.clone())
        }
        None => div().flex_none().text_color(muted).child("No runs yet"),
    };
    let mut said: Vec<AnyElement> = vec![reference];
    if !issue.labels.is_empty() {
        said.push(
            div()
                .h_flex()
                .flex_wrap()
                .gap_1()
                .children(issue.labels.iter().map(|label| chip(label.clone(), cx)))
                .into_any_element(),
        );
    }
    said.push(run.into_any_element());
    let changed = issue.updated.max(issue.created);
    if changed > 0 {
        said.push(
            div()
                .flex_none()
                .text_color(muted)
                .child(format!(
                    "updated {}",
                    onehand_core::rel_time(onehand_core::issues::now(), changed)
                ))
                .into_any_element(),
        );
    }
    said.extend(priority(&issue.body).map(|priority| {
        div()
            .flex_none()
            .text_color(muted)
            .child(format!("Priority: {priority}"))
            .into_any_element()
    }));
    // What a body written from a template left out: advice, never a refusal,
    // and nothing at all for a body written its own way.
    said.extend(lacking(&issue.body, &doing.templates).map(|lacks| {
        div()
            .flex_none()
            .text_color(muted)
            .child(lacks.said())
            .into_any_element()
    }));
    let mut facts = div()
        .h_flex()
        .flex_wrap()
        .items_center()
        .gap_1()
        .flex_none()
        .px_4()
        .pb_3()
        .border_b_1()
        .border_color(cx.theme().border)
        .text_xs()
        .child(state_badge(open, cx));
    for fact in said {
        facts = facts.child(div().text_color(muted).child("·")).child(fact);
    }
    let conflict = conflict_view(issue, cx);

    let progress = work::progress_view(&doing, &next, cx);
    let left = work::left_view(&doing, cx);
    let before = doing.work.as_ref().and_then(|w| work::before_view(w, cx));
    let history = history(root, issue, doing.history_open, cx);
    let review = doing.review;

    // Under the title and its facts everything scrolls as one: the body, then
    // what it names, what the work left, what came before and the history,
    // so none of them is pinned over the description it follows.
    let content = div()
        .id(("issue-content", number))
        .flex_1()
        .min_h_0()
        .v_flex()
        .overflow_y_scroll()
        .children(conflict)
        .children(progress)
        // Below where the work stands, pushing the body down: a person
        // opened it, so it is not content moving under them.
        .children(review)
        .child(match &body {
            Some((parsed, _)) => div()
                .max_w(BODY_MEASURE)
                .px_4()
                .py_3()
                .text_size(BODY_TEXT)
                .child(
                    TextView::new(parsed)
                        .selectable(true)
                        .style(body_style(window.rem_size(), cx))
                        .on_link_click(on_link(cx.entity().downgrade())),
                )
                .into_any_element(),
            None => div()
                .px_4()
                .py_3()
                .text_sm()
                .text_color(muted)
                .child("No description")
                .into_any_element(),
        })
        .children(body.and_then(|(_, files)| referenced(files, cx)))
        .children(left)
        .children(before)
        .child(history);

    div()
        .flex_1()
        .min_w_0()
        .h_full()
        .v_flex()
        .child(header)
        .child(facts)
        .child(content)
        .into_any_element()
}

/// Whether the issue is open or closed, as a badge: its own mark and word on
/// the success tint while open, on the neutral fill once closed.
fn state_badge(open: bool, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let (icon, fill, ink, word) = if open {
        let Tint { fill, ink } = tint(theme.green, cx);
        (Icon::new(AppIcon::CircleDot), fill, ink, "Open")
    } else {
        (
            Icon::new(IconName::CircleCheck),
            theme.secondary,
            theme.secondary_foreground,
            "Closed",
        )
    };
    div()
        .h_flex()
        .flex_none()
        .items_center()
        .gap_1()
        .px_1p5()
        .rounded_full()
        .bg(fill)
        .text_color(ink)
        .child(icon.xsmall())
        .child(word)
        .into_any_element()
}

/// Everything that happened to the issue, oldest first under its arrival,
/// folded under one line counting it until opened: each with how long ago,
/// the date and minute on hover, and for one a session took up, a way to open
/// that session. The latest ones only, with the cut said.
fn history(
    root: &Path,
    issue: &LocalIssue,
    open: bool,
    cx: &mut Context<IssuesView>,
) -> AnyElement {
    let muted = cx.theme().muted_foreground;
    let now = onehand_core::issues::now();
    let arrived = (issue.created > 0).then(|| (issue.created, issue.arrival(), None));
    let entries: Vec<(u64, String, Option<String>)> = arrived
        .into_iter()
        .chain(
            issue
                .notes
                .iter()
                .map(|note| (note.at, note.text.clone(), note.session.clone())),
        )
        .collect();
    let key = (root.to_path_buf(), issue.number);
    let count = match entries.len() {
        1 => "History · 1 event".to_string(),
        n => format!("History · {n} events"),
    };
    let left_out = entries.len().saturating_sub(HISTORY_SHOWN);
    div()
        .flex_none()
        .v_flex()
        .gap_1()
        .px_4()
        .py_3()
        .border_t_1()
        .border_color(cx.theme().border)
        .child(
            div()
                .id("issue-history")
                .h_flex()
                .items_center()
                .gap_1()
                .text_xs()
                .text_color(muted)
                .cursor_pointer()
                .child(
                    Icon::new(if open {
                        IconName::ChevronDown
                    } else {
                        IconName::ChevronRight
                    })
                    .xsmall(),
                )
                .child(count)
                .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                    view.history_open = match view.history_open.take() {
                        Some(shown) if shown == key => None,
                        _ => Some(key.clone()),
                    };
                    cx.notify();
                })),
        )
        .when(open && left_out > 0, |list| {
            list.child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child(format!("… {left_out} earlier not shown")),
            )
        })
        .when(open, |list| {
            list.children(entries.into_iter().skip(left_out).enumerate().map(
                |(i, (at, text, session))| {
                    div()
                        .h_flex()
                        .items_start()
                        .gap_2()
                        .text_xs()
                        .child(
                            div()
                                .id(("issue-history-at", i))
                                .flex_none()
                                .text_color(muted)
                                .tooltip(move |window, cx| {
                                    Tooltip::new(moment(at)).build(window, cx)
                                })
                                .child(onehand_core::rel_time(now, at)),
                        )
                        .child(div().flex_1().min_w_0().child(text))
                        .children(session.map(|session| {
                            action(("issue-history-session", i))
                                .xsmall()
                                .ghost()
                                .label("Open session")
                                .tooltip("Show the conversation that took this issue up")
                                .on_click(cx.listener(move |view, _: &ClickEvent, window, cx| {
                                    view.open_session(session.clone(), window, cx)
                                }))
                        }))
                },
            ))
        })
        .into_any_element()
}

/// `secs` since the epoch as a date and minute in the machine's own time zone,
/// which is the one the person reading it lives in.
fn moment(secs: u64) -> String {
    i64::try_from(secs)
        .ok()
        .and_then(|secs| chrono::DateTime::from_timestamp(secs, 0))
        .map(|at| {
            at.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_default()
}

/// The first line under a *Priority* heading in `body`, list marker and bold
/// taken off, or `None` where the body has no such section or it is empty.
fn priority(body: &str) -> Option<String> {
    let heading = |line: &str| line.trim_start().starts_with('#');
    let mut lines = body.lines();
    lines.find(|line| {
        heading(line)
            && line
                .trim()
                .trim_start_matches('#')
                .trim()
                .trim_end_matches(':')
                .eq_ignore_ascii_case("priority")
    })?;
    let first = lines
        .take_while(|line| !heading(line))
        .map(str::trim)
        .find(|line| !line.is_empty())?;
    let first = first
        .strip_prefix("- ")
        .or_else(|| first.strip_prefix("* "))
        .or_else(|| first.strip_prefix("+ "))
        .unwrap_or(first)
        .replace("**", "")
        .replace("__", "");
    let first = first.trim();
    (!first.is_empty()).then(|| first.to_string())
}

/// What a person has to decide about an issue both sides changed: each field in
/// conflict, as it reads here and as it reads on the forge, and the two ways
/// out. `None` for an issue with nothing to decide.
fn conflict_view(issue: &LocalIssue, cx: &mut Context<IssuesView>) -> Option<AnyElement> {
    let link = issue.link.as_ref()?;
    let theirs = link.conflict.as_ref()?;
    let ours = issue.snapshot();
    let number = issue.number;
    let forge = link.connector.clone();
    let open = |open: bool| if open { "open" } else { "closed" }.to_string();
    let lines: Vec<String> = sync::merge(&link.base, &ours, theirs)
        .conflicts
        .into_iter()
        .map(|field| match field {
            sync::Field::Title => {
                format!("Title — here: {}; on {forge}: {}", ours.title, theirs.title)
            }
            sync::Field::State => format!(
                "State — here: {}; on {forge}: {}",
                open(ours.open),
                open(theirs.open)
            ),
            sync::Field::Description => {
                let (here, there) = (excerpt(&ours.body), excerpt(&theirs.body));
                if here == there {
                    // Both begin alike and part further on, past what the line
                    // shows: said, or the two would read as one.
                    format!("Description — the two differ further on; here it begins: {here}")
                } else {
                    format!("Description — here: {here}\non {forge}: {there}")
                }
            }
        })
        .collect();
    let ink = status_ink(cx).warning;
    Some(
        div()
            .flex_none()
            .v_flex()
            .gap_1()
            .mx_2()
            .my_1()
            .p_2()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(ink)
            .child(
                div()
                    .text_xs()
                    .text_color(ink)
                    .child(format!("Changed here and on {forge} since the last sync:")),
            )
            .children(lines.into_iter().map(|line| div().text_xs().child(line)))
            .child(
                div()
                    .h_flex()
                    .gap_2()
                    .justify_end()
                    .child(
                        action("issue-keep-mine")
                            .xsmall()
                            .ghost()
                            .label("Keep mine")
                            .tooltip(format!("Send what it says here to {forge}"))
                            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                                view.resolve(number, true, cx)
                            })),
                    )
                    .child(
                        action("issue-take-theirs")
                            .xsmall()
                            .ghost()
                            .label(format!("Take {forge}'s"))
                            .tooltip("Replace what it says here with the other side")
                            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                                view.resolve(number, false, cx)
                            })),
                    ),
            )
            .into_any_element(),
    )
}

/// What pressing a link in the body does: a file it names opens in the editor,
/// and anything else goes to the system, as it would without this hook.
fn on_link(
    view: WeakEntity<IssuesView>,
) -> impl Fn(&SharedString, &ClickEvent, &mut Window, &mut App) + Send + Sync + 'static {
    move |url, _, window, cx| match url.strip_prefix(FILE_LINK) {
        Some(path) => {
            let _ = view.update(cx, |view, cx| view.open_path(path, window, cx));
        }
        None => cx.open_url(url),
    }
}

/// The files the body names, under it: each once, in the order it names them,
/// pressed to open in the editor. `None` for a body that names none.
fn referenced(files: Vec<String>, cx: &mut Context<IssuesView>) -> Option<AnyElement> {
    if files.is_empty() {
        return None;
    }
    let muted = cx.theme().muted_foreground;
    let left_out = files.len().saturating_sub(FILES_SHOWN);
    Some(
        div()
            .flex_none()
            .v_flex()
            .gap_0p5()
            .px_4()
            .py_3()
            .border_t_1()
            .border_color(cx.theme().border)
            .child(div().text_xs().text_color(muted).child("Referenced files"))
            .children(
                files
                    .into_iter()
                    .take(FILES_SHOWN)
                    .enumerate()
                    .map(|(i, path)| {
                        let open = path.clone();
                        div()
                            .id(("issue-file", i))
                            .h_flex()
                            .items_center()
                            .gap_1()
                            .min_w_0()
                            .px_1()
                            .rounded(cx.theme().radius)
                            .cursor_pointer()
                            .hover(|row| row.bg(cx.theme().list_hover))
                            .text_xs()
                            .font_family(cx.theme().mono_font_family.clone())
                            .child(Icon::new(IconName::File).xsmall().text_color(muted))
                            .child(div().min_w_0().truncate().child(path))
                            .on_click(cx.listener(move |view, _: &ClickEvent, window, cx| {
                                view.open_path(&open, window, cx)
                            }))
                    }),
            )
            .when(left_out > 0, |list| {
                list.child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child(format!("… {left_out} more not shown")),
                )
            })
            .into_any_element(),
    )
}

/// Body styling. The renderer sizes its headings from an absolute pixel base,
/// which the panel's rem-base zoom cannot reach by itself, so the base in force
/// is written in by hand, and each level is scaled from it here: the
/// renderer's own scale tops out at twice the body, which over an issue reads
/// as a page title shouting from inside the description.
fn body_style(rem: gpui::Pixels, cx: &App) -> TextViewStyle {
    let theme = cx.theme();
    let mut style = TextViewStyle::default()
        .code_block(
            gpui::StyleRefinement::default()
                .p(CODE_INSET)
                .text_size(CODE_TEXT)
                .bg(theme.muted),
        )
        // Inline code on the well, the quiet fill, rather than the renderer's
        // own fallback to the selected one, which in the dark palette is a
        // slab louder than the prose around it. The renderer styles inline
        // code through a highlight, which carries colour and background but
        // no font family and no padding, so those two are out of reach here.
        .inline_code(HighlightStyle {
            background_color: Some(theme.muted),
            ..HighlightStyle::default()
        })
        .heading_font_size(|level, rem| {
            rem * match level {
                1 => HEADING_1.0,
                2 => HEADING_2.0,
                _ => BODY_TEXT.0,
            }
        });
    style.heading_base_font_size = rem;
    style
}

/// The start of a description, for the line that sets two of them side by
/// side: enough to tell them apart, bounded so a long one does not push the
/// choice off the panel.
fn excerpt(text: &str) -> String {
    const SHOWN: usize = 160;
    match text.char_indices().nth(SHOWN) {
        Some((at, _)) => format!("{}…", &text[..at]),
        None if text.is_empty() => "(empty)".to_string(),
        None => text.to_string(),
    }
}

#[cfg(test)]
mod tests;
