//! Rendering a conversation to a Markdown document.
//!
//! It reads the whole conversation and draws nothing, so it lives with the
//! model: an export that dropped a block in one front end and not another would
//! be the same bug reported twice.

use crate::acp::ToolContent;
use crate::chat::model::{Chat, ChatItem};

/// Render the whole conversation (history + live) to a Markdown document.
pub fn export_markdown(chat: &Chat) -> String {
    let mut out = String::new();
    out.push_str("# onehand conversation\n\n");
    for item in chat.history.iter().chain(chat.items.iter()) {
        match item {
            ChatItem::User(u) => {
                out.push_str("## You\n\n");
                out.push_str(&u.text);
                for attachment in &u.attachments {
                    out.push_str(&format!("\n\n_📎 {}_", attachment.path.display()));
                }
                out.push_str("\n\n");
            }
            ChatItem::Agent(md) => {
                out.push_str("## Agent\n\n");
                out.push_str(&md.source);
                out.push_str("\n\n");
            }
            ChatItem::Thought(th) => {
                out.push_str("> _thinking_ ");
                out.push_str(&th.md.source.replace('\n', " "));
                out.push_str("\n\n");
            }
            ChatItem::Tool(t) => {
                let tc = &t.call;
                out.push_str(&format!(
                    "**Tool: {}** ({})\n\n",
                    tc.title,
                    tc.status.as_str()
                ));
                for c in &tc.content {
                    match c {
                        ToolContent::Text(t) => push_fenced(&mut out, "", t),
                        ToolContent::Diff { path, new, .. } => {
                            push_fenced(&mut out, "diff", &format!("# {path}\n{new}"))
                        }
                        ToolContent::Terminal(id) => {
                            let o = chat
                                .terminals
                                .get(id)
                                .map(|v| v.output.as_str())
                                .unwrap_or("");
                            push_fenced(&mut out, "", o)
                        }
                        ToolContent::Image(_) => out.push_str("_(image)_\n\n"),
                    }
                }
            }
            ChatItem::Plan(p) => {
                out.push_str("**Todos**\n\n");
                for e in &p.entries {
                    let mark = match e.status {
                        crate::acp::PlanStatus::Completed => "x",
                        _ => " ",
                    };
                    out.push_str(&format!("- [{mark}] {}\n", e.content));
                }
                out.push('\n');
            }
            ChatItem::Notice { text, .. } => {
                out.push_str(&format!("_{text}_\n\n"));
            }
            ChatItem::Permission(_) => {}
            // Only an answered question is worth exporting — the prompt plus
            // what the user picked, as a one-line Q&A.
            ChatItem::Ask(a) => {
                if let Some(answer) = &a.resolved {
                    out.push_str(&format!("**{}** → {answer}\n\n", a.req.message));
                }
            }
        }
    }
    out
}

/// Append `body` as a fenced code block whose fence is longer than any
/// backtick run *inside* it — tool output containing ``` would otherwise
/// close the fence early and garble the rest of the export.
fn push_fenced(out: &mut String, info: &str, body: &str) {
    let mut longest = 0usize;
    let mut run = 0usize;
    for c in body.chars() {
        if c == '`' {
            run += 1;
            longest = longest.max(run);
        } else {
            run = 0;
        }
    }
    let fence = "`".repeat((longest + 1).max(3));
    out.push_str(&fence);
    out.push_str(info);
    out.push('\n');
    out.push_str(body);
    out.push('\n');
    out.push_str(&fence);
    out.push_str("\n\n");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::model::UserMsg;

    /// Output holding a fence of its own does not end the block early.
    ///
    /// This is the one piece of arithmetic in the export, and getting it wrong
    /// does not fail loudly: the document still opens, and everything after the
    /// run of backticks inside the block is silently read as prose. A command
    /// that printed a markdown file is enough to trigger it.
    #[test]
    fn a_fence_is_longer_than_any_backtick_run_inside_it() {
        let mut out = String::new();
        push_fenced(&mut out, "text", "before\n````\nafter");
        assert!(
            out.starts_with("`````text\n"),
            "five backticks for a body holding four, got {out:?}"
        );
        assert!(
            out.trim_end().ends_with("`````"),
            "closed at the same width"
        );

        // Nothing unusual inside still gets markdown's ordinary three.
        let mut plain = String::new();
        push_fenced(&mut plain, "", "no fence here");
        assert!(plain.starts_with("```\n"), "got {plain:?}");

        // A run broken by other characters is not one long run.
        let mut split = String::new();
        push_fenced(&mut split, "", "`` x ``");
        assert!(split.starts_with("```\n"), "got {split:?}");
    }

    /// History and live items are one document, in that order.
    #[test]
    fn the_export_runs_history_then_live() {
        let mut chat = Chat::default();
        chat.history = vec![ChatItem::User(UserMsg::text("asked first"))];
        chat.items = vec![ChatItem::User(UserMsg::text("asked second"))];
        let out = export_markdown(&chat);
        let first = out.find("asked first").expect("history is exported");
        let second = out.find("asked second").expect("live items are exported");
        assert!(first < second, "history comes first");
    }
}
