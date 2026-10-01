use super::RailTab;
use super::project::{auto_status, project_hint, run_on, runs_more_than_one_agent};
use super::row::project_key;
use super::session::{LABEL_SHAPE_CAP, session_label, signal_hint};
use super::workspace::new_session_hint;
use crate::chat::pane::SessionSignal;
use gpui::SharedString;
use onehand_core::config::PanelLayout;
use std::path::Path;

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

/// Every signal says something, and no two say the same thing.
///
/// The words are the half of the signal that needs no learning and that
/// survives a reader who cannot separate the tints, so a mark that shares
/// its neighbour's sentence has given that half back. A blank one has given
/// it up entirely.
#[test]
fn each_signal_names_itself_and_no_two_alike() {
    let all = [
        SessionSignal::Lost,
        SessionSignal::AwaitingUser,
        SessionSignal::Busy,
        SessionSignal::UnseenTurn,
    ];
    let hints: Vec<&str> = all.iter().copied().map(signal_hint).collect();
    for (signal, hint) in all.iter().zip(&hints) {
        assert!(!hint.trim().is_empty(), "{signal:?} says nothing");
    }
    let mut unique = hints.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), hints.len(), "two signals say the same thing");
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

/// Two tabs, two names, and every tab in the list has one — a segmented
/// control with a blank half is one nobody can press on purpose.
#[test]
fn each_tab_names_itself_and_no_two_alike() {
    let labels: Vec<&str> = RailTab::ALL.iter().map(|tab| tab.label()).collect();
    assert!(labels.iter().all(|label| !label.trim().is_empty()));
    let mut unique = labels.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), labels.len());
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
    let branch = SharedString::from("feat/rail");
    let hint = project_hint("onehand", Some(&branch), 3, None, &path);
    assert_eq!(
        hint,
        vec![
            SharedString::from("onehand"),
            SharedString::from("Branch: feat/rail"),
            SharedString::from("3 changed files"),
            path.clone(),
        ]
    );

    // One file is one file. A count is read as prose here, so the plural
    // has to follow it.
    assert!(
        project_hint("onehand", Some(&branch), 1, None, &path)
            .contains(&SharedString::from("1 changed file"))
    );
}

/// A switched-on project's row says so, says which issue a run is on, and
/// says in words why when nothing can come of the switch.
#[test]
fn a_project_says_whether_its_issues_are_worked_and_which_one_is() {
    assert_eq!(auto_status(false, None, "auto", None), None);
    let on = auto_status(true, None, "auto", None).unwrap();
    assert_eq!(on.badge.as_ref(), "auto");
    assert!(on.line.contains("`auto`") && !on.stuck);
    let working = auto_status(true, Some((46, false)), "auto", None).unwrap();
    assert_eq!(working.badge.as_ref(), "auto · #46");
    assert!(working.line.contains("#46") && working.line.contains("working"));
    // A run waiting on a card is not said to be working.
    let waiting = auto_status(true, Some((46, true)), "auto", None).unwrap();
    assert_eq!(waiting.badge.as_ref(), "auto · #46 waiting");
    assert!(waiting.line.contains("waiting") && !waiting.line.contains("working"));
    // Switched off mid-run: the run already going is still said.
    assert!(auto_status(false, Some((46, false)), "auto", None).is_some());
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
    let hint = project_hint("p", None, 0, Some(&on.line), &path);
    assert_eq!(hint.last(), Some(&path));
    assert!(hint.contains(&on.line));
}

/// A project's row names a run on its own issues, and the working one
/// ahead of a waiting one: the older run is usually the waiting one, and
/// it would otherwise hide the run that is actually going.
#[test]
fn a_project_row_names_its_own_run_and_prefers_the_working_one() {
    let (here, there) = (std::path::Path::new("/p"), std::path::Path::new("/q"));
    assert_eq!(run_on([(there, 9, false)], here), None);
    assert_eq!(
        run_on([(here, 3, true), (there, 9, false)], here),
        Some((3, true))
    );
    assert_eq!(
        run_on([(here, 3, true), (here, 5, false)], here),
        Some((5, false))
    );
}

/// A project that is neither a repository nor changed is the row the old
/// tooltip could not reach at all: it hung off the git suffix, and that
/// row draws none. It still has a name and a path, and those are what the
/// hover is for.
#[test]
fn a_plain_folder_still_answers_on_hover() {
    let path = SharedString::from("/work/notes");
    let hint = project_hint("notes", None, 0, None, &path);
    assert_eq!(hint, vec![SharedString::from("notes"), path]);
}

/// The footnote naming the agent is there to tell two rows apart, so the
/// question is asked of the rows. A project whose sessions all run one
/// agent gets a column of identical words out of it and nothing else —
/// which is what it was doing, because the count it used was the agent
/// menu's and a second entry there is enough to mark up every project in
/// the workspace.
#[test]
fn one_agent_across_a_projects_sessions_is_not_worth_saying() {
    assert!(!runs_more_than_one_agent(std::iter::empty()));
    assert!(!runs_more_than_one_agent(["Claude Code"].into_iter()));
    assert!(!runs_more_than_one_agent(
        ["Claude Code", "Claude Code", "Claude Code"].into_iter()
    ));
}

/// And the other half: where they disagree it is the only thing on the row
/// that says which is which.
#[test]
fn two_agents_in_one_project_are_worth_saying() {
    assert!(runs_more_than_one_agent(
        ["Claude Code", "Mock UI"].into_iter()
    ));
    // The disagreement can be anywhere in the list, not only at its head.
    assert!(runs_more_than_one_agent(
        ["Claude Code", "Claude Code", "Mock UI"].into_iter()
    ));
}
