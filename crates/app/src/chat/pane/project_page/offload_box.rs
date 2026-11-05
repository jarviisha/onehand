//! The project page's boxes on an offloaded project: which side holds its
//! code and when the code last moved, and, while a copy back stands stopped,
//! what changed here and the two ways to settle it. On a branch, the one box
//! says what the host's branch has to merge, and sends this folder's commit
//! up when asked.

use super::super::{ChatPane, ChatPaneEvent};
use crate::controls::{Refuses as _, Short as _};
use crate::offload::{BranchState, Moving, Offload};
use crate::page_parts::page_card;
use gpui::{App, Context, Div, IntoElement as _, ParentElement as _, Styled as _, div};
use gpui_component::button::ButtonVariants as _;
use gpui_component::{ActiveTheme as _, StyledExt as _};
use onehand_core::host::offload::Changes;
use onehand_core::host::wire::Holder;
use onehand_core::workspace::HostName;
use std::path::Path;

/// How many of a stopped copy's changes the box lists.
const STOPPED_CAP: usize = 50;

/// What the box says first: where the code is, or where it is going.
fn holder_line(o: &Offload) -> String {
    let host = &o.host;
    match (o.moving, o.state.as_ref()) {
        (Some(Moving::Up), _) => format!("Sending the code to {host}…"),
        (Some(Moving::Back), _) => format!("Bringing the code back from {host}…"),
        (None, None) => "Reading what was kept…".into(),
        (None, Some(state)) => match (state.key, &state.stopped) {
            (Holder::Laptop, _) => {
                format!("The code is here; it goes to {host} with the next prompt")
            }
            (Holder::Host, Some(_)) => {
                format!("{host} keeps the code until bringing it back is settled")
            }
            (Holder::Host, None) => crate::offload::lock_line(host),
        },
    }
}

/// A stopped copy's changes in a person's words, files first: a ref by the
/// branch or tag it is, the index and the settings by what they hold.
fn changed_lines(changes: &Changes) -> Vec<String> {
    let refs = changes.refs.iter().map(|r| match r.as_str() {
        "index" => "What is staged".to_string(),
        "config" => "The repository's settings".to_string(),
        "HEAD" => "HEAD".to_string(),
        other => match (
            other.strip_prefix("refs/heads/"),
            other.strip_prefix("refs/tags/"),
        ) {
            (Some(branch), _) => format!("Branch {branch}"),
            (_, Some(tag)) => format!("Tag {tag}"),
            _ => other.to_string(),
        },
    });
    changes
        .files
        .iter()
        .map(|f| f.display().to_string())
        .chain(refs)
        .collect()
}

/// What the host's `branch` has that `HEAD` does not, once counted.
fn branch_line(branch: &str, ahead: Option<usize>) -> Option<String> {
    Some(match ahead? {
        0 => format!("{branch} has nothing this branch does not"),
        1 => format!("{branch} has 1 commit this branch does not"),
        n => format!("{branch} has {n} commits this branch does not"),
    })
}

/// What the branch box draws, read off the project's entry.
struct BranchBox {
    host: HostName,
    branch: BranchState,
    moving: Option<Moving>,
    failed: Option<String>,
}

/// The box *On <host>* for a project offloaded on `branch`: no key, so
/// only what its branch has to merge and the way to send this folder up.
fn branch_box(path: &Path, on: BranchBox, cx: &mut Context<ChatPane>) -> Div {
    let BranchBox {
        host,
        branch,
        moving,
        failed,
    } = on;
    let name = branch.name.clone();
    let muted = cx.theme().muted_foreground;
    let line = |text: String, cx: &App| {
        div()
            .text_sm()
            .text_color(cx.theme().foreground)
            .child(text)
    };
    let danger = crate::theme::status_ink(cx).danger;
    let update = crate::controls::action("offload-update-host")
        .secondary()
        .short()
        .label("Update host from laptop")
        .loading(moving == Some(Moving::Up))
        .refuses((moving == Some(Moving::Back)).then_some("The branch is being fetched"))
        .on_click(cx.listener({
            let path = path.to_path_buf();
            move |_: &mut ChatPane, _, _, cx| {
                cx.emit(ChatPaneEvent::UpdateHost(path.clone()));
            }
        }));
    page_card(format!("On {host}"), None, None, cx).child(
        div()
            .v_flex()
            .gap_1()
            .child(line(
                format!("Sessions on {host} work on {name}; this folder stays yours"),
                cx,
            ))
            .children(branch_line(&name, branch.ahead).map(|text| line(text, cx)))
            .children(
                [branch.refused.clone(), failed]
                    .into_iter()
                    .flatten()
                    .map(|why| div().text_xs().text_color(danger).child(why)),
            )
            .child(div().h_flex().pt_2().child(update))
            .child(div().text_xs().text_color(muted).child(format!(
                "Merges this folder's last commit into {name}; what is not committed stays here"
            ))),
    )
}

/// The box *On <host>*, for the offloaded project at `path`, and the box
/// *Bringing the code back stopped* under it while that stands.
pub(super) fn offload_box(path: &Path, cx: &mut Context<ChatPane>) -> Option<Div> {
    let o = crate::offload::get(cx, path)?;
    if let Some(branch) = o.branch.clone() {
        let on = BranchBox {
            host: o.host.clone(),
            branch,
            moving: o.moving,
            failed: o.failed.clone(),
        };
        return Some(branch_box(path, on, cx));
    }
    let muted = cx.theme().muted_foreground;
    let line = |text: String, cx: &App| {
        div()
            .text_sm()
            .text_color(cx.theme().foreground)
            .child(text)
    };
    let stopped = o
        .state
        .as_ref()
        .filter(|s| s.key == Holder::Host)
        .and_then(|s| s.stopped.clone());
    let ink = match stopped.is_some() {
        true => crate::theme::status_ink(cx).warning,
        false => cx.theme().foreground,
    };
    let moved = o.state.as_ref().map(|s| s.moved).filter(|at| *at > 0);
    let now = onehand_core::chat::now_secs();
    let failed = o.failed.clone();
    let danger = crate::theme::status_ink(cx).danger;
    let (host, moving) = (o.host.clone(), o.moving);
    let on = page_card(format!("On {host}"), None, None, cx).child(
        div()
            .v_flex()
            .gap_1()
            .child(line(holder_line(o), cx).text_color(ink))
            .children(moved.map(|at| {
                div()
                    .text_xs()
                    .text_color(muted)
                    .child(format!("Last moved {}", onehand_core::rel_time(now, at)))
            }))
            .children(failed.map(|why| div().text_xs().text_color(danger).child(why))),
    );
    let stopped_box = stopped.map(|changes| {
        let all = changed_lines(&changes);
        let left_out = all.len().saturating_sub(STOPPED_CAP);
        let busy = moving.is_some().then_some("The code is on its way");
        let check = crate::controls::action("offload-check-again")
            .secondary()
            .short()
            .label("Check again")
            .loading(moving == Some(Moving::Back))
            .refuses(busy)
            .on_click(cx.listener({
                let path = path.to_path_buf();
                move |_: &mut ChatPane, _, _, cx| {
                    let path = path.clone();
                    cx.spawn(async move |_, cx| crate::offload::check_again(cx, path).await)
                        .detach();
                }
            }));
        let take = crate::controls::action("offload-take-host-code")
            .secondary()
            .short()
            .label("Take the host's code…")
            .refuses(busy)
            .on_click(cx.listener({
                let path = path.to_path_buf();
                move |_: &mut ChatPane, _, _, cx| {
                    cx.emit(ChatPaneEvent::TakeHostCode(path.clone()));
                }
            }));
        page_card("Bringing the code back stopped", None, None, cx)
            .child(line(
                format!("These changed on this machine while {host} held the code:"),
                cx,
            ))
            .child(
                div().v_flex().children(
                    all.into_iter()
                        .take(STOPPED_CAP)
                        .map(|item| line(item, cx).min_w_0().truncate()),
                ),
            )
            .children((left_out > 0).then(|| {
                div()
                    .text_xs()
                    .text_color(muted)
                    .child(format!("{left_out} more not shown"))
            }))
            .child(
                div()
                    .h_flex()
                    .gap_2()
                    .child(check)
                    .child(take)
                    .into_any_element(),
            )
    });
    Some(div().v_flex().gap_4().child(on).children(stopped_box))
}

#[cfg(test)]
mod tests {
    use super::{branch_line, changed_lines};
    use onehand_core::host::offload::Changes;

    #[test]
    fn a_stopped_copy_says_files_then_refs_in_words() {
        let changes = Changes {
            files: vec!["src/a.rs".into()],
            refs: vec![
                "HEAD".into(),
                "refs/heads/main".into(),
                "refs/tags/v1".into(),
                "refs/remotes/origin/main".into(),
                "index".into(),
                "config".into(),
            ],
        };
        assert_eq!(
            changed_lines(&changes),
            [
                "src/a.rs",
                "HEAD",
                "Branch main",
                "Tag v1",
                "refs/remotes/origin/main",
                "What is staged",
                "The repository's settings",
            ]
        );
    }

    #[test]
    fn the_branch_box_counts_what_is_left_to_merge() {
        let b = "onehand/box";
        assert_eq!(branch_line(b, None), None);
        assert_eq!(
            branch_line(b, Some(0)).as_deref(),
            Some("onehand/box has nothing this branch does not")
        );
        assert_eq!(
            branch_line(b, Some(1)).as_deref(),
            Some("onehand/box has 1 commit this branch does not")
        );
        assert_eq!(
            branch_line(b, Some(3)).as_deref(),
            Some("onehand/box has 3 commits this branch does not")
        );
    }
}
