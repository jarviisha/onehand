use super::list::{chip, identity, plain_tag};
use super::mentions::FILE_LINK;
use super::work;
use super::{FILES_SHOWN, HISTORY_SHOWN, IssuesView};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, ClickEvent, Context, Entity, HighlightStyle, InteractiveElement as _,
    IntoElement, ParentElement, Rems, SharedString, StatefulInteractiveElement as _, Styled,
    WeakEntity, Window, div, rems,
};
use gpui_component::tab::{Tab, TabBar};
use gpui_component::tag::Tag;
use gpui_component::text::{TextView, TextViewState, TextViewStyle};
use gpui_component::tooltip::Tooltip;
use gpui_component::{ActiveTheme, Icon, IconName, Sizable as _, StyledExt};
use onehand_core::issues::template::{IssueTemplate, lacking};
use onehand_core::issues::{LocalIssue, sync};
use onehand_core::task::Group;
use onehand_core::task::work::{IssueWork, PrSeen};
use onehand_plugin_host::Quiet as _;
use onehand_plugin_host::tokens::{BAR_H, CODE_TEXT, PAGE_PROSE};
use onehand_plugin_host::{AppIcon, Tint, action, status_ink, tint};
use priority::{priority, priority_ink};
use std::path::Path;

/// A description's headings on the type scale: the first at `text_lg`, the
/// second at `text_base`, the rest at the body's `text_sm`, told apart by
/// weight. Numbers rather than the style methods because the renderer asks a
/// size of each level.
const HEADING_1: Rems = rems(1.125);
const HEADING_2: Rems = rems(1.);
const HEADING_REST: Rems = rems(0.875);

/// The gutter inside a fenced block, wider than a row's so the code reads as
/// set in its well rather than on its edge.
const CODE_INSET: Rems = rems(0.75);

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
    /// The issue is being sent to its forge.
    pub(super) publishing: bool,
    /// The templates the issue's project offers, which its body is read
    /// against.
    pub(super) templates: Vec<IssueTemplate>,
    /// What the issue's project is called, for the trail over it.
    pub(super) project: SharedString,
    /// Which of the issue's sections shows under its head.
    pub(super) tab: IssueTab,
    /// On the Issues page, where the issue stands in the list and the way to
    /// close it, at the end of the trail.
    pub(super) nav: Option<AnyElement>,
}

/// The sections an issue's head switches between: what it says and what its
/// work left behind, and everything that happened to it.
#[derive(Clone, Copy, Default, PartialEq)]
pub(super) enum IssueTab {
    #[default]
    Description,
    Activity,
}

/// One issue, read: a trail naming it with its actions at the end; its state,
/// project and labels; the title; one line of facts; then, before any of
/// what it says, what it waits on (a conflict, where its work stands, the
/// review); and under two tabs its description with what its work left, and
/// its activity.
pub(super) fn issue_view(
    root: &Path,
    issue: &LocalIssue,
    body: Option<(Entity<TextViewState>, Vec<String>)>,
    publish_to: Option<&'static str>,
    mut doing: Doing<'_>,
    window: &mut Window,
    cx: &mut Context<IssuesView>,
) -> AnyElement {
    let nav = doing.nav.take();
    let review = doing.review.take();
    let number = issue.number;
    let theme = cx.theme();
    let muted = theme.muted_foreground;
    let border = theme.border;
    let next = work::next_for(issue, &doing);
    let named = issue
        .reference()
        .map_or_else(|| format!("#{number}"), str::to_string);

    // The trail: the project, then the issue, and its actions at the end of
    // the same bar, so what to do is found where the issue is named.
    let trail = div()
        .h_flex()
        .items_center()
        .gap_2()
        .flex_none()
        .min_h(BAR_H)
        .px_4()
        .border_b_1()
        .border_color(border)
        .child(
            div()
                .h_flex()
                .flex_1()
                .min_w_0()
                .items_center()
                .gap_1()
                .text_xs()
                .text_color(muted)
                .child(div().min_w_0().truncate().child(doing.project.clone()))
                .child(Icon::new(IconName::ChevronRight).xsmall())
                .child(
                    div()
                        .flex_none()
                        .font_family(theme.mono_font_family.clone())
                        .child(named.clone()),
                ),
        )
        .child(work::actions(root, issue, &doing, &next, publish_to, cx))
        .children(nav);

    // What it is at a glance: open or closed, where it lives, what it is
    // filed under.
    let tags = div()
        .h_flex()
        .flex_wrap()
        .items_center()
        .gap_1p5()
        .child(state_badge(issue.open, cx))
        .child(plain_tag(doing.project.to_string(), cx))
        .children(issue.labels.iter().map(|label| chip(label.clone(), cx)));

    // The title, the largest words on the issue: the whole of it, wrapped.
    let title = div().text_xl().font_semibold().child(issue.title.clone());

    // Its facts on one line: the reference (to the forge, where it is
    // linked), the project, when it changed, how its work stands, how urgent
    // the body says it is, and what a template-written body left out.
    let reference = match (issue.reference(), &issue.link) {
        (Some(reference), Some(link)) => div()
            .id("issue-reference")
            .flex_none()
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
    let fact = |text: String| div().flex_none().child(text).into_any_element();
    let mut said: Vec<AnyElement> = vec![reference, fact(doing.project.to_string())];
    let changed = issue.changed();
    if changed > 0 {
        said.push(fact(format!(
            "Updated {}",
            onehand_core::rel_time(onehand_core::issues::now(), changed)
        )));
    }
    said.push(match doing.work.as_ref().map(|work| &work.work) {
        Some(work) => {
            let ink = if work.group.needs_attention() {
                status_ink(cx).warning
            } else if work.group == Group::Running {
                cx.theme().link
            } else {
                muted
            };
            div()
                .flex_none()
                .text_color(ink)
                .child(work.said.clone())
                .into_any_element()
        }
        None => fact("No runs yet".to_string()),
    });
    // Inked by how hard it presses, so only what cannot wait draws the eye.
    said.extend(priority(&issue.body).map(|priority| {
        div()
            .flex_none()
            .text_color(priority_ink(&priority, cx))
            .child(format!("Priority: {priority}"))
            .into_any_element()
    }));
    // Advice, never a refusal, and nothing at all for a body written its own
    // way.
    said.extend(lacking(&issue.body, &doing.templates).map(|lacks| fact(lacks.said().to_string())));
    let mut facts = div()
        .h_flex()
        .flex_wrap()
        .items_center()
        .gap_1()
        .text_xs()
        .text_color(muted);
    for (i, said) in said.into_iter().enumerate() {
        if i > 0 {
            facts = facts.child(div().child("·"));
        }
        facts = facts.child(said);
    }

    let events = usize::from(issue.created > 0) + issue.notes.len();
    let tabs = [IssueTab::Description, IssueTab::Activity];
    let at = tabs.iter().position(|tab| *tab == doing.tab).unwrap_or(0);
    let shown = tabs[at];
    let view = cx.entity();
    let picker = {
        TabBar::new(("issue-tabs", number))
            .underline()
            .small()
            .selected_index(at)
            .children(tabs.iter().map(|tab| {
                let (name, count) = match tab {
                    IssueTab::Description => ("Description", None),
                    IssueTab::Activity => ("Activity", Some(events)),
                };
                Tab::new().label(name).when_some(count, |tab, count| {
                    tab.suffix(div().text_xs().text_color(muted).child(count.to_string()))
                })
            }))
            .on_click(move |at, _, cx| {
                let tab = tabs[*at];
                view.update(cx, |view, cx| {
                    view.issue_tab = tab;
                    cx.notify();
                })
            })
    };

    let head = div()
        .v_flex()
        .gap_3()
        .px_4()
        .pt_4()
        .child(tags)
        .child(title)
        .child(facts)
        .child(picker);

    let section = match shown {
        IssueTab::Description => div()
            .v_flex()
            .child(match &body {
                Some((parsed, _)) => div()
                    .px_4()
                    .py_3()
                    .text_sm()
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
            // What the work left follows what the issue asked for.
            .children(work::left_view(&doing, cx))
            .children(doing.work.as_ref().and_then(|w| work::before_view(w, cx))),
        IssueTab::Activity => div().v_flex().child(activity(issue, cx)),
    };

    // Under the trail everything scrolls as one, so a long title or a tall
    // review never pins the description out of reach. What the issue waits
    // on comes before its tabs, whichever of them is showing.
    let content = div()
        .v_flex()
        .child(head)
        .children(conflict_view(issue, cx))
        .children(work::progress_view(&doing, &next, cx))
        // Below where the work stands, pushing the rest down: a person opened
        // it, so it is not content moving under them.
        .children(review)
        .child(section);

    div()
        .flex_1()
        .min_w_0()
        .h_full()
        .v_flex()
        .child(trail)
        // The scroll spans the column, so its bar stays on the column's edge
        // while what it scrolls keeps to the issue's width.
        .child(
            div()
                .id(("issue-content", number))
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .child(centred(content)),
        )
        .into_any_element()
}

/// `inner` at most [`PAGE_PROSE`] wide, centred with equal gutters either
/// side: on a wide page the issue reads as a column of its own, and on a
/// narrow one it simply takes the width.
fn centred(inner: gpui::Div) -> gpui::Div {
    div()
        .w_full()
        .h_flex()
        .justify_center()
        .child(inner.w_full().max_w(PAGE_PROSE))
}

/// Whether the issue is open or closed, as a badge: its own mark and word on
/// the success tint while open, on the neutral fill once closed.
fn state_badge(open: bool, cx: &App) -> Tag {
    let theme = cx.theme();
    let (icon, Tint { fill, ink }, word) = if open {
        (Icon::new(AppIcon::CircleDot), tint(theme.green, cx), "Open")
    } else {
        let neutral = Tint {
            fill: theme.secondary,
            ink: theme.secondary_foreground,
        };
        (Icon::new(IconName::CircleCheck), neutral, "Closed")
    };
    Tag::custom(fill, ink, fill)
        .small()
        .rounded(cx.theme().radius_lg)
        .flex_none()
        .gap_1()
        .child(icon.xsmall())
        .child(word)
}

/// Everything that happened to the issue, oldest first under its arrival:
/// each with how long ago, the date and minute on hover, and for one a
/// session took up, a way to open that session. The latest ones only, with
/// the cut said.
fn activity(issue: &LocalIssue, cx: &mut Context<IssuesView>) -> AnyElement {
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
    let left_out = entries.len().saturating_sub(HISTORY_SHOWN);
    div()
        .flex_none()
        .v_flex()
        .gap_1()
        .px_4()
        .py_3()
        .when(entries.is_empty(), |list| {
            list.child(div().text_xs().text_color(muted).child("Nothing yet"))
        })
        .when(left_out > 0, |list| {
            list.child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child(format!("… {left_out} earlier not shown")),
            )
        })
        .children(
            entries
                .into_iter()
                .skip(left_out)
                .enumerate()
                .map(|(i, (at, text, session))| {
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
                                .quiet(cx)
                                .label("Open session")
                                .tooltip("Show the conversation that took this issue up")
                                .on_click(cx.listener(move |view, _: &ClickEvent, window, cx| {
                                    view.open_session(session.clone(), window, cx)
                                }))
                        }))
                }),
        )
        .into_any_element()
}

/// `secs` since the epoch as a date and minute in the machine's own time zone,
/// which is the one the person reading it lives in.
pub(super) fn moment(secs: u64) -> String {
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
                            .quiet(cx)
                            .label("Keep mine")
                            .tooltip(format!("Send what it says here to {forge}"))
                            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                                view.resolve(number, true, cx)
                            })),
                    )
                    .child(
                        action("issue-take-theirs")
                            .xsmall()
                            .quiet(cx)
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
                _ => HEADING_REST.0,
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

pub(super) mod priority;
