use super::complete::TriggerSpot;
use super::complete::trigger_spot;
use super::{Draft, highlight};
use onehand_core::attachment::{AttachmentSource, StagedAttachment};
use std::path::PathBuf;

#[test]
fn the_command_trigger_goes_to_the_front_and_the_mention_stays_put() {
    assert_eq!(trigger_spot('/', "", 0), TriggerSpot::Insert(0));
    assert_eq!(
        trigger_spot('/', "review this for me", 18),
        TriggerSpot::Insert(0),
        "what was written becomes the command's argument"
    );
    assert_eq!(
        trigger_spot('/', "/compact", 8),
        TriggerSpot::Reuse(1),
        "one slash is enough"
    );
    assert_eq!(trigger_spot('@', "look at ", 8), TriggerSpot::Insert(8));
    assert_eq!(trigger_spot('@', "look at ", 99), TriggerSpot::Insert(8));
}

#[test]
fn a_selection_past_the_end_falls_back_to_the_last_row() {
    assert_eq!(highlight(0, 3), Some(0));
    assert_eq!(highlight(2, 3), Some(2));
    assert_eq!(highlight(7, 3), Some(2));
    assert_eq!(
        highlight(0, 0),
        None,
        "nothing to highlight, nothing to accept"
    );
}

#[test]
fn the_pinned_header_names_what_the_popup_is() {
    use super::Overlay;
    use super::popup::popup_title;
    use super::presentation::Row;
    use onehand_core::completion::TriggerKind;

    let headed = |name: &str| Row {
        group: Some(gpui::SharedString::from(name.to_string())),
        ..Row::default()
    };

    // The trigger is the only thing that knows what a completion is for,
    // and a lone `@` does not say it.
    assert_eq!(
        popup_title(&Overlay::Completion, Some(TriggerKind::File), &[]),
        "Mention a file"
    );
    assert_eq!(
        popup_title(&Overlay::Completion, Some(TriggerKind::Command), &[]),
        "Run a command"
    );
    // One group: the list takes its name, and the heading that repeated it
    // is dropped by the caller.
    assert_eq!(
        popup_title(&Overlay::Mode, None, &[headed("Mode"), Row::default()]),
        "Mode"
    );
    // Several: naming it after the first would be naming a third of it.
    assert_eq!(
        popup_title(
            &Overlay::Options,
            None,
            &[headed("Model"), Row::default(), headed("Effort")]
        ),
        "Settings"
    );
}

#[test]
fn a_list_is_capped_by_the_panel_and_never_below_its_floor() {
    use super::popup::POPUP_MIN_H;
    use super::popup_room;
    use gpui::px;

    let rem = px(16.);
    let row = super::popup::POPUP_ROW_H.to_pixels(rem);

    // **The whole-row bound is on the scrolling box, so it is exact
    // whatever the chrome comes to.** It used to be on the surface, with
    // one hand-measured constant subtracted first — and this test asserted
    // that arithmetic against the same constant, so it passed while the
    // fold landed mid-row for every popup whose chrome was not exactly
    // that number. Measured here against each shape the popup actually
    // takes.
    {
        let chrome = super::popup::popup_chrome();
        for panel in [800., 500., 300., 0.] {
            let room = popup_room(px(panel), px(120.), rem);
            let list = super::popup::popup_list_h(room, rem, chrome);
            assert_eq!(list % row, px(0.), "at {panel}: the fold cuts a row");
            assert!(
                list <= row * super::popup::POPUP_MAX_ROWS,
                "at {panel}: more rows than a list shows at once"
            );
            assert!(list >= row, "at {panel}: no room for a row");
        }
    }

    assert!(
        popup_room(px(800.), px(120.), rem) < px(800. - 120. - 16.),
        "the row cap has to be the binding one on a tall panel or it says nothing"
    );
    assert_eq!(
        popup_room(px(200.), px(180.), rem),
        POPUP_MIN_H.to_pixels(rem),
        "a squeezed panel bottoms out rather than collapsing to one row"
    );
}

#[test]
fn an_attachment_alone_is_not_an_empty_draft() {
    let draft = Draft {
        text: String::new(),
        attachments: vec![StagedAttachment::inspect(
            PathBuf::from("/tmp/notes.md"),
            AttachmentSource::Picker,
        )],
    };
    assert!(!draft.is_empty());
    assert!(Draft::default().is_empty());
}
