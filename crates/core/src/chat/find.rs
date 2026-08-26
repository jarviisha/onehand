//! Transcript search and Markdown export.
//!
//! Both read the whole conversation and neither draws anything, so they live
//! with the model: a find that matched different items in the two front ends,
//! or an export that dropped a block in one of them, would be the same bug
//! reported twice.

use crate::acp::ToolContent;
use crate::chat::model::{Chat, ChatItem, TranscriptItemId};

/// The searchable text of a transcript item (used by the find bar).
pub fn item_search_text(chat: &Chat, item: &ChatItem) -> String {
    match item {
        ChatItem::User(u) => {
            let mut searchable = u.text.clone();
            for attachment in &u.attachments {
                searchable.push(' ');
                searchable.push_str(&attachment.name);
            }
            searchable
        }
        ChatItem::Agent(md) => md.source.clone(),
        ChatItem::Thought(th) => th.md.source.clone(),
        ChatItem::Notice { text, .. } => text.clone(),
        ChatItem::Permission(p) => p.req.title.clone(),
        ChatItem::Ask(a) => a.req.message.clone(),
        ChatItem::Plan(p) => p
            .entries
            .iter()
            .map(|e| e.content.as_str())
            .collect::<Vec<_>>()
            .join(" "),
        ChatItem::Tool(t) => {
            let tc = &t.call;
            let mut s = tc.title.clone();
            if let Some(description) = &tc.description {
                s.push(' ');
                s.push_str(description);
            }
            for c in &tc.content {
                match c {
                    ToolContent::Text(t) => {
                        s.push(' ');
                        s.push_str(t);
                    }
                    ToolContent::Diff { path, new, .. } => {
                        s.push(' ');
                        s.push_str(path);
                        s.push(' ');
                        s.push_str(new);
                    }
                    ToolContent::Terminal(id) => {
                        if let Some(terminal) = chat.terminals.get(id) {
                            s.push(' ');
                            s.push_str(&terminal.output);
                        }
                    }
                    ToolContent::Image(_) => {}
                }
            }
            s
        }
    }
}

/// Positions of transcript items matching `query` (case-insensitive), across the
/// whole rendered sequence: read-only `history` first (positions `0..history.len`),
/// then the live `items`. These are *render positions*, not the live-items index
/// used for interaction — history is searchable but read-only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TranscriptMatch {
    pub target: TranscriptItemId,
    /// Absolute item position in the history + live sequence. Used only to
    /// choose a bounded render page and a relative scroll destination.
    pub position: usize,
}

pub fn compute_matches(chat: &Chat, query: &str) -> Vec<TranscriptMatch> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Vec::new();
    }
    chat.history
        .iter()
        .chain(chat.items.iter())
        .enumerate()
        .filter(|(_, it)| item_search_text(chat, it).to_lowercase().contains(&q))
        .map(|(position, _)| TranscriptMatch {
            target: if position < chat.history.len() {
                TranscriptItemId::History(position)
            } else {
                TranscriptItemId::Live(position - chat.history.len())
            },
            position,
        })
        .collect()
}

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
    use crate::acp::{ToolCall, ToolKind, ToolStatus};
    use crate::chat::model::{TermView, ToolItem, UserMsg};

    #[test]
    fn matches_span_history_then_items() {
        let mut chat = Chat::default();
        chat.history = vec![ChatItem::User(UserMsg::text("hello from history"))];
        chat.items = vec![
            ChatItem::User(UserMsg::text("a live message")),
            ChatItem::notice("hello again"),
        ];
        // "hello" hits history[0] (pos 0) and items[1] (pos 1 + 1 = 2).
        let m = compute_matches(&chat, "hello");
        assert_eq!(m[0].target, TranscriptItemId::History(0));
        assert_eq!(m[1].target, TranscriptItemId::Live(1));
    }

    #[test]
    fn matches_are_case_insensitive_and_empty_query_is_none() {
        let mut chat = Chat::default();
        chat.items = vec![ChatItem::User(UserMsg::text("FooBar"))];
        assert_eq!(compute_matches(&chat, "foobar")[0].position, 0);
        assert!(compute_matches(&chat, "   ").is_empty());
    }

    #[test]
    fn live_terminal_output_is_searchable() {
        let mut chat = Chat::default();
        chat.terminals.insert(
            "term-1".into(),
            TermView {
                output: "compile finished successfully".into(),
                exited: true,
                exit_code: Some(0),
            },
        );
        chat.items.push(ChatItem::Tool(ToolItem::new(ToolCall {
            id: "tool-1".into(),
            title: "cargo build".into(),
            description: None,
            kind: ToolKind::Execute,
            status: ToolStatus::Completed,
            content: vec![ToolContent::Terminal("term-1".into())],
        })));
        assert_eq!(
            compute_matches(&chat, "finished successfully")[0].target,
            TranscriptItemId::Live(0)
        );
    }
}
