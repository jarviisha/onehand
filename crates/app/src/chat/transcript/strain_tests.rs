//! A parked card's answers in a narrow pane and a zoomed one: they stand on
//! one line while they fit, and past that they wrap whole, never cut, since
//! a cut grant is one nobody can read the reach of.

use crate::shell::rig::strain::{
    NARROWEST, WIDE, assert_inside, assert_one_line, bounds, same, selector, width,
};
use crate::shell::rig::{folder, nowhere, shell_on, shown_session, zoom_pane};
use gpui::{Bounds, Entity, Pixels, TestAppContext, VisualTestContext};
use onehand_core::acp::{AcpEvent, Elicitation, PermissionOption, PermissionRequest, ToolKind};
use onehand_core::config::PanelLayout;
use onehand_core::workspace::Workspace;

/// How many steps of the pane's zoom reach 150%.
const TO_150: usize = 5;

/// A window whose pane is at its least beside the narrowest rail, a session
/// shown in it, and the agent's channel.
fn pane<'a>(
    name: &str,
    cx: &'a mut TestAppContext,
) -> (
    Entity<crate::shell::Shell>,
    futures::channel::mpsc::Sender<AcpEvent>,
    &'a mut VisualTestContext,
) {
    let dir = folder(name);
    let mut ws = Workspace::seeded("/tmp");
    ws.layout.rail_w = PanelLayout::RAIL_MIN;
    let (_, shell, cx) = shell_on(cx, dir.join("tasks"), vec![nowhere()], ws);
    let (_, events, requests) = shown_session(&shell, "/tmp".into(), nowhere(), cx);
    std::mem::forget(requests);
    (shell, events, cx)
}

fn option(id: &str, name: &str, kind: &str) -> PermissionOption {
    PermissionOption {
        id: id.into(),
        name: name.into(),
        kind: kind.into(),
    }
}

fn ask_permission(
    options: Vec<PermissionOption>,
    events: &mut futures::channel::mpsc::Sender<AcpEvent>,
) {
    events
        .try_send(AcpEvent::Permission(PermissionRequest {
            rpc_id: serde_json::json!(1),
            tool_call_id: None,
            kind: ToolKind::Execute,
            title: "cargo build --release".into(),
            options,
            child: None,
            locations: Vec::new(),
        }))
        .unwrap();
}

/// The answers as the app words them: one of each kind.
fn worded() -> Vec<PermissionOption> {
    vec![
        option("once", "Allow this once", "allow_once"),
        option(
            "always",
            "Always allow commands like this one",
            "allow_always",
        ),
        option("deny", "Deny", "reject_once"),
    ]
}

/// Answers the app cannot word, two of one kind, so each shows the agent's
/// own name for it.
fn long_worded() -> Vec<PermissionOption> {
    vec![
        option(
            "once",
            "Allow cargo build in this folder, this time only",
            "allow_once",
        ),
        option(
            "once-all",
            "Allow every cargo command in this folder, this time only",
            "allow_once",
        ),
        option(
            "deny",
            "Deny, and tell the agent to find another way",
            "reject_once",
        ),
        option(
            "deny-all",
            "Deny every command it asks for until the turn ends",
            "reject_once",
        ),
    ]
}

/// The foot and its first `n` answers, as drawn.
fn foot(
    prefix: &str,
    n: usize,
    cx: &mut VisualTestContext,
) -> (Bounds<Pixels>, Vec<Bounds<Pixels>>) {
    let foot = bounds(cx, selector(format!("{prefix}-foot")));
    let answers = (0..n)
        .map(|i| bounds(cx, selector(format!("{prefix}-answer-{i}"))))
        .collect();
    (foot, answers)
}

fn one_line_where_they_fit(prefix: &str, n: usize, cx: &mut VisualTestContext) {
    for w in [NARROWEST, WIDE] {
        width(cx, w);
        let (foot, answers) = foot(prefix, n, cx);
        assert_one_line(&answers, &format!("the answers at {w:?}"));
        for answer in answers {
            assert_inside(answer, foot, "an answer");
        }
    }
}

/// At the pane's least, each answer is as tall and as wide as it is in a
/// wide window at the same zoom, so none was cut or broken inside itself,
/// and each stays in the foot.
fn whole_where_they_do_not(
    prefix: &str,
    n: usize,
    shell: &Entity<crate::shell::Shell>,
    zoomed: bool,
    cx: &mut VisualTestContext,
) {
    if zoomed {
        zoom_pane(shell, TO_150, cx);
    }
    width(cx, WIDE);
    let (_, roomy) = foot(prefix, n, cx);
    width(cx, NARROWEST);
    let (foot, answers) = foot(prefix, n, cx);
    for (i, (answer, roomy)) in answers.iter().zip(&roomy).enumerate() {
        assert!(
            same(answer.size.height, roomy.size.height)
                && same(answer.size.width, roomy.size.width),
            "answer {i} was cut or broken: {answer:?}, against {roomy:?} in a wide window"
        );
        assert_inside(*answer, foot, "an answer");
    }
}

#[gpui::test]
fn a_cards_answers_as_worded_stand_on_one_line(cx: &mut TestAppContext) {
    let (_, mut events, cx) = pane("strain-perm-worded", cx);
    ask_permission(worded(), &mut events);
    cx.run_until_parked();
    one_line_where_they_fit("perm", 3, cx);
}

#[gpui::test]
fn answers_that_do_not_fit_wrap_whole(cx: &mut TestAppContext) {
    let (shell, mut events, cx) = pane("strain-perm-zoomed", cx);
    ask_permission(worded(), &mut events);
    cx.run_until_parked();
    whole_where_they_do_not("perm", 3, &shell, true, cx);
}

#[gpui::test]
fn answers_worded_long_wrap_whole(cx: &mut TestAppContext) {
    let (shell, mut events, cx) = pane("strain-perm-long", cx);
    ask_permission(long_worded(), &mut events);
    cx.run_until_parked();
    whole_where_they_do_not("perm", 4, &shell, false, cx);
}

fn ask_question(events: &mut futures::channel::mpsc::Sender<AcpEvent>) {
    let ask: Elicitation = serde_json::from_value(serde_json::json!({
        "rpc_id": 7, "tool_call_id": null, "message": "One thing", "child": null,
        "fields": [{
            "key": "one", "title": "What should the build be called?", "description": null,
            "kind": "Text", "custom_key": null,
        }],
    }))
    .unwrap();
    events.try_send(AcpEvent::Elicitation(ask)).unwrap();
}

#[gpui::test]
fn a_question_cards_answers_as_worded_stand_on_one_line(cx: &mut TestAppContext) {
    let (_, mut events, cx) = pane("strain-ask-worded", cx);
    ask_question(&mut events);
    cx.run_until_parked();
    one_line_where_they_fit("ask", 2, cx);
}

#[gpui::test]
fn a_question_cards_answers_that_do_not_fit_wrap_whole(cx: &mut TestAppContext) {
    let (shell, mut events, cx) = pane("strain-ask-zoomed", cx);
    ask_question(&mut events);
    cx.run_until_parked();
    whole_where_they_do_not("ask", 2, &shell, true, cx);
}
