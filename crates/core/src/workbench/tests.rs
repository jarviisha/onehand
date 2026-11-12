use super::*;

const EDITOR: WorkbenchModeSpec =
    WorkbenchModeSpec::element(ModeId::new("editor"), "Editor").on_host();
const MARKDOWN: WorkbenchModeSpec = WorkbenchModeSpec::element(ModeId::new("markdown"), "Markdown");
const ISSUES: WorkbenchModeSpec = WorkbenchModeSpec::element(ModeId::new("issues"), "Issues");

fn showing() -> Showing {
    Showing::new(vec![MARKDOWN, EDITOR, ISSUES])
}

/// A switch to a host's folder, with a mode showing not offered there,
/// shows the first that is; a switch keeping the mode showing offered moves
/// nothing.
#[test]
fn a_switch_shows_the_first_mode_offered_when_the_one_showing_is_not() {
    let mut showing = showing();
    assert_eq!(showing.pick(ISSUES.id), Some(2));
    assert_eq!(showing.set_root(true), Some(1));
    assert!(showing.on_host());
    assert_eq!(showing.set_root(false), None);
    assert_eq!(showing.active(), 1);
}

/// A file opened is asked of the modes offered, in strip order, until one
/// takes it, which shows; none taking it moves nothing.
#[test]
fn a_file_opens_in_the_first_offered_mode_that_takes_it() {
    let mut showing = showing();
    let mut asked = Vec::new();
    let took = showing.opened(|place| {
        asked.push(place);
        place >= 1
    });
    assert_eq!(took, Some(1));
    assert_eq!(asked, [0, 1]);
    showing.set_root(true);
    let mut asked = Vec::new();
    assert_eq!(
        showing.opened(|place| {
            asked.push(place);
            false
        }),
        None
    );
    assert_eq!(asked, [1], "only what is offered on a host is asked");
    assert_eq!(showing.active(), 1);
}

/// A mode picked shows, the one showing included; one not offered, or any
/// while a refusal stands, is refused, and so is a file opened.
#[test]
fn a_pick_shows_it_unless_refused_or_not_offered() {
    let mut showing = showing();
    assert_eq!(showing.pick(MARKDOWN.id), Some(0));
    assert_eq!(
        showing.pick(MARKDOWN.id),
        Some(0),
        "picked again, told again"
    );
    showing.set_root(true);
    assert_eq!(showing.pick(MARKDOWN.id), None);
    showing.refuse(true);
    assert_eq!(showing.pick(EDITOR.id), None);
    assert_eq!(showing.opened(|_| true), None);
    showing.refuse(false);
    assert_eq!(showing.pick(EDITOR.id), Some(1));
}
