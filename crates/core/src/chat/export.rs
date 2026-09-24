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
