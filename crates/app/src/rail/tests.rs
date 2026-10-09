use super::model::{
    Filter, Item, Row, SESSION_CAP, Shape, age_label, attention_count, badge, meta, order, rows,
    toggle_attention, visible,
};
use super::project::{GitLine, auto_status, project_hint, run_on};
use super::row::project_key;
use super::session::{LABEL_SHAPE_CAP, session_label, signal_hint, signal_word};
use super::workspace::new_session_hint;
use crate::chat::pane::SessionSignal;
use gpui::SharedString;
use onehand_core::config::PanelLayout;
use std::path::Path;

const PROJECTS: [&str; 3] = ["atlas-api", "atlas-billing", "docs-site"];

/// Six sessions over three projects, in tree order, each `uid` its start.
fn seed() -> Vec<Item> {
    use SessionSignal::*;
    let s = |root, session, uid, title: &str, signal, age| Item {
        root,
        session,
        uid,
        title: title.into(),
        agent: "claude".into(),
        signal,
        age,
    };
    vec![
        s(0, 0, 1, "Inspect dashboard", Some(AwaitingUser), 4),
        s(0, 1, 2, "Fix flaky retry test", Some(Busy), 1),
        s(0, 2, 6, "Audit interface", Some(Failed), 25),
        s(0, 3, 3, "Rename the retry keys", None, 2 * 60 * 24),
        s(1, 0, 4, "Reconcile ledger totals", Some(UnseenTurn), 90),
        s(1, 1, 5, "Backfill ledger exports", Some(Busy), 12),
    ]
}

fn shape<'a>(
    filter: Filter,
    query: &'a str,
    open: &'a dyn Fn(usize) -> bool,
    uncapped: &'a dyn Fn(Option<usize>) -> bool,
) -> Shape<'a> {
    Shape {
        projects: &PROJECTS,
        order: &[0, 1, 2],
        filter,
        query,
        open,
        uncapped,
    }
}

#[test]
fn a_filter_keeps_what_it_says() {
    let s = seed();
    assert_eq!(
        visible(&s, &PROJECTS, Filter::ByProject, ""),
        vec![0, 1, 2, 3, 4, 5]
    );
    // Started order, by uid.
    assert_eq!(
        visible(&s, &PROJECTS, Filter::All, ""),
        vec![0, 1, 3, 4, 5, 2]
    );
    assert_eq!(
        visible(&s, &PROJECTS, Filter::NeedsAttention, ""),
        vec![0, 2]
    );
    // The latest change of state first: 1m, 4m, 12m, 25m, 90m, two days.
    assert_eq!(
        visible(&s, &PROJECTS, Filter::Recent, ""),
        vec![1, 0, 5, 2, 4, 3]
    );
}

#[test]
fn the_query_matches_a_title_or_a_project_ignoring_case() {
    let s = seed();
    assert_eq!(
        visible(&s, &PROJECTS, Filter::ByProject, "  RETRY "),
        vec![1, 3]
    );
    assert_eq!(
        visible(&s, &PROJECTS, Filter::ByProject, "billing"),
        vec![4, 5]
    );
    assert!(visible(&s, &PROJECTS, Filter::NeedsAttention, "retry").is_empty());
}

#[test]
fn attention_counts_input_failures_and_lost_agents() {
    let mut s = seed();
    assert_eq!(attention_count(&s), 2);
    s[1].signal = Some(SessionSignal::Lost);
    assert_eq!(attention_count(&s), 3);
}

#[test]
fn the_meta_line_keeps_one_order() {
    use SessionSignal::*;
    assert_eq!(meta(Some(Busy), "claude", 1), "Running · claude · 1m");
    assert_eq!(
        meta(Some(AwaitingUser), "claude", 4),
        "Needs input · claude · waiting 4m"
    );
    assert_eq!(meta(Some(Failed), "codex", 25), "Failed · codex · 25m");
    assert_eq!(meta(Some(Lost), "codex", 0), "Disconnected · codex · now");
    assert_eq!(meta(None, "claude", 2 * 60 * 24), "Idle · claude · 2d");
}

#[test]
fn an_age_is_said_in_its_largest_whole_unit() {
    assert_eq!(age_label(0), "now");
    assert_eq!(age_label(59), "59m");
    assert_eq!(age_label(60), "1h");
    assert_eq!(age_label(60 * 24 - 1), "23h");
    assert_eq!(age_label(60 * 24 * 3), "3d");
}

#[test]
fn a_folded_project_shows_its_most_urgent_state() {
    let mut s = seed();
    assert_eq!(badge(&s, 0), Some(SessionSignal::Failed));
    s[2].signal = None;
    assert_eq!(badge(&s, 0), Some(SessionSignal::AwaitingUser));
    s[0].signal = Some(SessionSignal::Lost);
    assert_eq!(badge(&s, 0), Some(SessionSignal::Lost));
    // Done and running only: the run shows.
    assert_eq!(badge(&s, 1), Some(SessionSignal::Busy));
    s[5].signal = None;
    // Done is not waiting on anyone and nothing runs.
    assert_eq!(badge(&s, 1), None);
    assert_eq!(badge(&s, 2), None);
}

#[test]
fn the_attention_chip_toggles_back_to_the_filter_it_replaced() {
    assert_eq!(
        toggle_attention(Filter::Recent, Filter::Recent),
        Filter::NeedsAttention
    );
    assert_eq!(
        toggle_attention(Filter::NeedsAttention, Filter::Recent),
        Filter::Recent
    );
}

#[test]
fn the_tree_draws_open_projects_with_their_sessions_and_ends_on_add_project() {
    let s = seed();
    let open = |root: usize| root != 0;
    assert_eq!(
        rows(&s, &shape(Filter::ByProject, "", &open, &|_| false)),
        vec![
            Row::Project {
                root: 0,
                open: false
            },
            Row::Gap,
            Row::Project {
                root: 1,
                open: true
            },
            Row::Session {
                item: 4,
                flat: false
            },
            Row::Session {
                item: 5,
                flat: false
            },
            Row::Gap,
            Row::Project {
                root: 2,
                open: true
            },
            Row::Empty,
            Row::AddProject,
        ]
    );
}

#[test]
fn a_search_opens_every_project_that_matches_and_drops_the_rest() {
    let s = seed();
    let rows = rows(
        &s,
        &shape(Filter::ByProject, "ledger", &|_| false, &|_| false),
    );
    assert_eq!(
        rows,
        vec![
            Row::Project {
                root: 1,
                open: true
            },
            Row::Session {
                item: 4,
                flat: false
            },
            Row::Session {
                item: 5,
                flat: false
            },
            Row::AddProject,
        ]
    );
    let none = rows_for(&s, "nothing like this");
    assert_eq!(none, vec![Row::NoMatch, Row::AddProject]);
}

fn rows_for(s: &[Item], query: &str) -> Vec<Row> {
    rows(s, &shape(Filter::ByProject, query, &|_| true, &|_| false))
}

#[test]
fn a_flat_list_names_no_projects_and_says_when_nothing_matches() {
    let s = seed();
    let rows = rows(
        &s,
        &shape(Filter::NeedsAttention, "", &|_| true, &|_| false),
    );
    assert_eq!(
        rows,
        vec![
            Row::Session {
                item: 0,
                flat: true
            },
            Row::Session {
                item: 2,
                flat: true
            },
            Row::AddProject,
        ]
    );
    let quiet: Vec<Item> = seed()
        .into_iter()
        .map(|mut item| {
            item.signal = None;
            item
        })
        .collect();
    assert_eq!(
        super::model::rows(
            &quiet,
            &shape(Filter::NeedsAttention, "", &|_| true, &|_| false)
        ),
        vec![Row::NoMatch, Row::AddProject]
    );
}

#[test]
fn a_group_past_the_cap_says_how_many_more_until_shown_whole() {
    let many: Vec<Item> = (0..SESSION_CAP + 2)
        .map(|i| Item {
            root: 0,
            session: i,
            uid: i as u64,
            title: format!("session {i}"),
            agent: "claude".into(),
            signal: None,
            age: 0,
        })
        .collect();
    let capped = rows(&many, &shape(Filter::ByProject, "", &|_| true, &|_| false));
    assert_eq!(order(&capped).len(), SESSION_CAP);
    assert!(capped.contains(&Row::More {
        group: Some(0),
        hidden: 2
    }));
    let whole = rows(
        &many,
        &shape(Filter::ByProject, "", &|_| true, &|group| group == Some(0)),
    );
    assert_eq!(order(&whole).len(), SESSION_CAP + 2);
    assert!(!whole.iter().any(|row| matches!(row, Row::More { .. })));
    let flat = rows(&many, &shape(Filter::All, "", &|_| true, &|_| false));
    assert!(flat.contains(&Row::More {
        group: None,
        hidden: 2
    }));
}

/// A project's row has to be named by the project, because the library
/// keys a row's expanded state by whatever name the row is rendered under
/// — and the name it hands out by default is the row's *position*, which
/// moves when a project is pinned or another one is removed. What moved
/// with it was the wrong project's expanded state.
///
/// The folder name is not enough on its own: two checkouts of one
/// repository are two projects sharing one folder name, and a worktree
/// made from the rail is exactly that.
#[test]
fn two_projects_with_one_folder_name_are_two_different_rows() {
    assert_ne!(
        project_key(Path::new("/work/alpha/onehand")),
        project_key(Path::new("/work/beta/onehand")),
    );
}

/// The other half: the same project is the same row wherever it is drawn,
/// which is what makes pinning safe to reorder the list.
#[test]
fn one_project_keeps_one_row_wherever_it_is_drawn() {
    assert_eq!(
        project_key(Path::new("/work/alpha/onehand")),
        project_key(Path::new("/work/alpha/onehand")),
    );
}

/// The button is one click and carries no visible target, so the project it
/// would start in has to be said somewhere. This is that somewhere.
#[test]
fn the_primary_action_names_the_project_it_would_start_in() {
    let hint = new_session_hint(Some("foxai-pos-web"), Some("Claude Code"));
    assert!(hint.contains("foxai-pos-web"), "{hint}");
    assert!(hint.contains("Claude Code"), "{hint}");
}

/// With no project there is nothing to start a session in, and a tooltip
/// promising one would be describing a click that cannot happen.
#[test]
fn with_no_project_the_hint_asks_for_one() {
    let hint = new_session_hint(None, Some("Claude Code"));
    assert!(hint.contains("Add a project"), "{hint}");
}

/// Every state says something on hover and is called something on its row,
/// and no two say the same thing: the words are the half of a mark that
/// survives a reader who cannot separate the inks.
#[test]
fn each_state_names_itself_and_no_two_alike() {
    use SessionSignal::*;
    let all = [Lost, Failed, AwaitingUser, Busy, UnseenTurn];
    for words in [
        all.iter()
            .map(|s| signal_hint(Some(*s)))
            .collect::<Vec<_>>(),
        all.iter().map(|s| signal_word(*s)).collect(),
    ] {
        assert!(words.iter().all(|w| !w.trim().is_empty()));
        let mut unique = words.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), words.len(), "two states say the same thing");
    }
}

#[test]
fn a_session_row_prefers_the_conversations_own_name() {
    assert_eq!(
        session_label(Some("Fix the login flow"), "Claude Code"),
        "Fix the login flow"
    );
}

/// Until a conversation has been prompted it has no name of its own, and a
/// blank row would be worse than a repeated one.
#[test]
fn an_unprompted_session_falls_back_to_its_agent() {
    assert_eq!(session_label(None, "Claude Code"), "Claude Code");
}

/// A first prompt is free text and users paste paragraphs into it. What
/// fits the row is the fade's business, in pixels — this bound is the cost
/// one, keeping a pasted essay from being shaped whole on every frame the
/// rail draws. It has to sit past what the widest rail can show, or the
/// ellipsis it writes would reach the screen and the fade would be a lie.
#[test]
fn a_long_title_is_bounded_for_cost_not_for_fit() {
    let label = session_label(Some(&"a".repeat(LABEL_SHAPE_CAP * 3)), "Claude Code");
    assert_eq!(label.chars().count(), LABEL_SHAPE_CAP);
    assert!(label.ends_with('…'));
}

/// The other end of that rope, and the load-bearing half: the cost bound
/// has to sit past what the widest rail can draw, or the ellipsis it
/// writes reaches the screen and the fade it replaced becomes a lie.
///
/// The width tests that used to hold this end went with `label_cap`. The
/// figure is a *narrowest*-glyph width rather than an average one, because
/// what has to be impossible is the cap biting first for any string at
/// all — a column of `i`s is the worst case and roughly this wide at the
/// size a rail row is drawn.
#[test]
fn the_cost_bound_can_never_be_the_visible_cut() {
    const NARROWEST_GLYPH: f32 = 4.;
    let widest = PanelLayout::RAIL_MAX;
    assert!(
        LABEL_SHAPE_CAP as f32 * NARROWEST_GLYPH >= widest,
        "{LABEL_SHAPE_CAP} characters can be drawn inside a {widest}px rail, \
             so the cost cap cuts a name the fade was supposed to"
    );
}

/// What a project row says on hover, in the order it says it.
///
/// Every line here is one the row itself cuts -- the name, the branch and
/// the count are all capped in the row, and the path is not on the row at
/// all -- so this is the only place any of them is readable whole. The
/// path goes last because it is the longest and the least often wanted,
/// and the count is written in words because a bare figure beside a branch
/// name reads as part of it.
#[test]
fn a_projects_hover_carries_every_part_the_row_cut() {
    let path = SharedString::from("/work/onehand");
    let git = GitLine {
        branch: "feat/rail".into(),
        changed: 3,
        ahead: 2,
        behind: 0,
    };
    let hint = project_hint("onehand", Some(&git), None, &path);
    assert_eq!(
        hint,
        vec![
            SharedString::from("onehand"),
            SharedString::from("Branch: feat/rail"),
            SharedString::from("3 uncommitted changes"),
            SharedString::from("2 commits ahead of the remote"),
            path.clone(),
        ]
    );

    // One is one. A count is read as prose here, so the plural follows it,
    // and a part at zero is not said at all.
    let one = GitLine {
        changed: 1,
        ahead: 0,
        behind: 1,
        ..git
    };
    let hint = project_hint("onehand", Some(&one), None, &path);
    assert!(hint.contains(&SharedString::from("1 uncommitted change")));
    assert!(hint.contains(&SharedString::from("1 commit behind the remote")));
    assert!(!hint.iter().any(|line| line.contains("ahead")));
}

/// A switched-on project's row says so, says which issue a run is on, and
/// says in words why when nothing can come of the switch.
#[test]
fn a_project_says_whether_its_issues_are_worked_and_which_one_is() {
    assert_eq!(auto_status(false, None, "auto", None), None);
    let on = auto_status(true, None, "auto", None).unwrap();
    assert_eq!(on.badge.as_ref(), "auto");
    assert!(on.line.contains("`auto`") && !on.stuck);
    let working = auto_status(true, Some(("#46", false)), "auto", None).unwrap();
    assert_eq!(working.badge.as_ref(), "auto · #46");
    assert!(working.line.contains("#46") && working.line.contains("working"));
    // A run waiting on a card is not said to be working.
    let waiting = auto_status(true, Some(("#46", true)), "auto", None).unwrap();
    assert_eq!(waiting.badge.as_ref(), "auto · #46 waiting");
    assert!(waiting.line.contains("waiting") && !waiting.line.contains("working"));
    // Switched off mid-run: the run already going is still said.
    assert!(auto_status(false, Some(("#46", false)), "auto", None).is_some());
    // On with nothing possible is stuck, and says why in its own words.
    let elsewhere = auto_status(
        true,
        None,
        "auto",
        Some("its remote is on gitlab.com".into()),
    )
    .unwrap();
    assert!(elsewhere.stuck && elsewhere.line.contains("gitlab.com"));
    // The hover carries the line, ahead of the path.
    let path = SharedString::from("/p");
    let hint = project_hint("p", None, Some(&on.line), &path);
    assert_eq!(hint.last(), Some(&path));
    assert!(hint.contains(&on.line));
}

/// A project's row names a run on its own issues, and the working one
/// ahead of a waiting one: the older run is usually the waiting one, and
/// it would otherwise hide the run that is actually going.
#[test]
fn a_project_row_names_its_own_run_and_prefers_the_working_one() {
    let (here, there) = (std::path::Path::new("/p"), std::path::Path::new("/q"));
    assert_eq!(run_on([(there, "#9", false)], here), None);
    assert_eq!(
        run_on([(here, "#3", true), (there, "#9", false)], here),
        Some(("#3", true))
    );
    assert_eq!(
        run_on([(here, "#3", true), (here, "#5", false)], here),
        Some(("#5", false))
    );
}

/// A project that is neither a repository nor changed is the row the old
/// tooltip could not reach at all: it hung off the git suffix, and that
/// row draws none. It still has a name and a path, and those are what the
/// hover is for.
#[test]
fn a_plain_folder_still_answers_on_hover() {
    let path = SharedString::from("/work/notes");
    let hint = project_hint("notes", None, None, &path);
    assert_eq!(hint, vec![SharedString::from("notes"), path]);
}

/// The Tasks row's pill is there only when something needs a person, and
/// then says how many.
#[test]
fn the_tasks_pill_shows_only_a_count_above_zero() {
    assert_eq!(super::attention_pill(0), None);
    assert_eq!(super::attention_pill(3).as_deref(), Some("3"));
}
