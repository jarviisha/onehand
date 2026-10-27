//! Translate ACP `session/update` notifications into a flat list of updates
//!. Pure — operates on `serde_json::Value` so it is
//! unit-testable without the transport. Phase 2 handles the message/thought/user
//! content streams; tool calls, plans, commands and selectors arrive in Phase 3.

use crate::acp::types::{
    BackgroundReport, ConfigChoice, ConfigOption, LimitStatus, PlanEntry, PlanStatus, RateLimit,
    SlashCommand, ToolCall, ToolCallUpdate, ToolContent, ToolKind, ToolStatus, Usage,
};
use serde_json::Value;
use std::sync::Arc;

/// A single parsed effect of a `session/update`.
#[derive(Debug, Clone, PartialEq)]
pub enum Update {
    /// A chunk of the agent's reply text (markdown).
    Agent(String),
    /// A chunk of the agent's reasoning.
    Thought(String),
    /// A chunk echoed from the user (resume replay).
    User(String),
    /// The adapter's own marker for a turn the person cut short.
    Interrupted,
    /// A new tool call.
    ToolCall(ToolCall),
    /// An update to an existing tool call.
    ToolUpdate(ToolCallUpdate),
    /// The agent (re)published its plan (Claude Code's TodoWrite).
    Plan(Vec<PlanEntry>),
    /// The agent's advertised slash commands (`/` completion).
    Commands(Vec<SlashCommand>),
    /// The current session mode changed.
    ModeChanged(String),
    /// The session's config options (model/effort/agent) were (re)published.
    ConfigOptions(Vec<ConfigOption>),
    /// Context fill and rate limit.
    Usage(Usage),
    /// Background work started or ended.
    Background(BackgroundReport),
}

/// Whether a user chunk is the adapter's own marker for a turn the person cut
/// short, which it sends in the person's voice: `[Request interrupted by
/// user]`, or `... for tool use]` when a tool was refused. Read as a user
/// chunk it would be drawn as something the person typed.
fn is_interruption(chunk: &str) -> bool {
    let chunk = chunk.trim();
    chunk.starts_with("[Request interrupted by user") && chunk.ends_with(']')
}

/// Parse the inner `update` object of a `session/update` notification.
pub(crate) fn parse_session_update(update: &Value) -> Vec<Update> {
    let kind = update
        .get("sessionUpdate")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let text = || extract_text(update.get("content"));
    match kind {
        "agent_message_chunk" => text().map(Update::Agent).into_iter().collect(),
        "agent_thought_chunk" => text().map(Update::Thought).into_iter().collect(),
        "user_message_chunk" => text()
            .map(|chunk| match is_interruption(&chunk) {
                true => Update::Interrupted,
                false => Update::User(chunk),
            })
            .into_iter()
            .collect(),
        "tool_call" => vec![Update::ToolCall(parse_tool_call(update))],
        "tool_call_update" => vec![Update::ToolUpdate(parse_tool_update(update))],
        "plan" => vec![Update::Plan(parse_plan(update))],
        "available_commands_update" => vec![Update::Commands(parse_commands(update))],
        "current_mode_update" => update
            .get("currentModeId")
            .and_then(Value::as_str)
            .map(|m| vec![Update::ModeChanged(m.to_string())])
            .unwrap_or_default(),
        "config_option_update" => vec![Update::ConfigOptions(parse_config_options(
            update.get("configOptions"),
        ))],
        "usage_update" => parse_usage(update).map(Update::Usage).into_iter().collect(),
        "async_task_spawned" => {
            let text = |k: &str| update.get(k).and_then(Value::as_str);
            let (Some(id), kind) = (text("asyncTaskId"), text("taskType").unwrap_or_default())
            else {
                return Vec::new();
            };
            let name = text("name").or(text("description")).unwrap_or(kind);
            vec![Update::Background(BackgroundReport::Spawned {
                id: id.into(),
                name: name.into(),
                kind: kind.into(),
            })]
        }
        "async_task_state_update" => {
            let ended = matches!(
                update.get("state").and_then(Value::as_str),
                Some("completed" | "failed" | "stopped")
            );
            update
                .get("asyncTaskId")
                .and_then(Value::as_str)
                .filter(|_| ended)
                .map(|id| {
                    vec![Update::Background(BackgroundReport::Ended {
                        id: id.into(),
                    })]
                })
                .unwrap_or_default()
        }
        // Progress ticks carry nothing the list shows.
        "async_task_progress" => Vec::new(),
        _ => Vec::new(),
    }
}

fn parse_usage(u: &Value) -> Option<Usage> {
    let used = u.get("used").and_then(Value::as_u64)?;
    let size = u.get("size").and_then(Value::as_u64)?;
    let meta = u.get("_meta");
    let limit = meta.and_then(|m| m.get("_claude/rateLimit")).and_then(|r| {
        let status = match r.get("status").and_then(Value::as_str)? {
            "allowed" => LimitStatus::Allowed,
            "allowed_warning" => LimitStatus::Warning,
            "rejected" => LimitStatus::Rejected,
            _ => return None,
        };
        Some(RateLimit {
            status,
            utilization: r
                .get("utilization")
                .and_then(Value::as_f64)
                .map(|f| f as f32),
            resets_at: r.get("resetsAt").and_then(Value::as_u64),
            kind: r
                .get("rateLimitType")
                .and_then(Value::as_str)
                .map(str::to_string),
        })
    });
    // A person's own turn ends with its prompt's answer; any other origin is
    // the agent waking itself.
    let cycle_end = meta
        .and_then(|m| m.get("_claude/origin"))
        .and_then(|o| o.get("kind"))
        .and_then(Value::as_str)
        .is_some_and(|k| k != "human");
    Some(Usage {
        used,
        size,
        limit,
        cycle_end,
    })
}

/// Parse a `configOptions` array (from `session/new` or `config_option_update`)
/// into the selectable groups. The `"mode"` group is dropped — session mode is
/// driven by the standard `modes` field + `session/set_mode`.
pub(crate) fn parse_config_options(options: Option<&Value>) -> Vec<ConfigOption> {
    options
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(parse_config_option)
                .filter(|o| o.id != "mode")
                .collect()
        })
        .unwrap_or_default()
}

fn parse_config_option(o: &Value) -> Option<ConfigOption> {
    let id = o.get("id").and_then(Value::as_str)?.to_string();
    let name = o
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or(&id)
        .to_string();
    let category = o
        .get("category")
        .and_then(Value::as_str)
        .map(str::to_string);
    let current = o
        .get("currentValue")
        .and_then(Value::as_str)
        .map(str::to_string);
    let choices = o
        .get("options")
        .and_then(Value::as_array)
        .map(|opts| {
            opts.iter()
                .filter_map(|c| {
                    let value = c.get("value").and_then(Value::as_str)?.to_string();
                    let name = c
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or(&value)
                        .to_string();
                    let description = c
                        .get("description")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|description| !description.is_empty())
                        .map(str::to_string);
                    Some(ConfigChoice {
                        value,
                        name,
                        description,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Some(ConfigOption {
        id,
        name,
        category,
        current,
        choices,
    })
}

/// Parse `available_commands_update` → the slash command list.
fn parse_commands(u: &Value) -> Vec<SlashCommand> {
    u.get("availableCommands")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|c| {
                    let name = c.get("name").and_then(Value::as_str)?.to_string();
                    let description = c
                        .get("description")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    Some(SlashCommand { name, description })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Parse a `plan` update's entries (`[{ content, status, priority }]`) — the
/// agent's checklist, republished in full on every change (TodoWrite).
fn parse_plan(u: &Value) -> Vec<PlanEntry> {
    u.get("entries")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|e| {
                    let content = e.get("content").and_then(Value::as_str)?.to_string();
                    let status =
                        PlanStatus::parse(e.get("status").and_then(Value::as_str).unwrap_or(""));
                    Some(PlanEntry { content, status })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Parse a full `tool_call` update.
fn parse_tool_call(u: &Value) -> ToolCall {
    let id = u
        .get("toolCallId")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let title = u
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("Tool call")
        .to_string();
    let kind = ToolKind::parse(u.get("kind").and_then(Value::as_str).unwrap_or(""));
    let status = ToolStatus::parse(u.get("status").and_then(Value::as_str).unwrap_or("pending"));
    ToolCall {
        id,
        title,
        description: tool_description(u),
        kind,
        status,
        content: parse_tool_content(u.get("content")),
    }
}

/// Parse a partial `tool_call_update` (only present fields change).
fn parse_tool_update(u: &Value) -> ToolCallUpdate {
    ToolCallUpdate {
        id: u
            .get("toolCallId")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        status: u
            .get("status")
            .and_then(Value::as_str)
            .map(ToolStatus::parse),
        title: u.get("title").and_then(Value::as_str).map(str::to_string),
        description: tool_description(u),
        content: u.get("content").map(|c| parse_tool_content(Some(c))),
    }
}

/// Claude Code deliberately keeps a Bash command in ACP's standard `title`
/// and publishes the model-authored natural-language description out of band.
/// Prefer that documented metadata, with `rawInput.description` as a fallback
/// for older adapter versions that exposed only the original Bash input.
fn tool_description(u: &Value) -> Option<String> {
    u.pointer("/_meta/claudeCode/title")
        .and_then(Value::as_str)
        .or_else(|| {
            u.get("rawInput")
                .and_then(|input| input.get("description"))
                .and_then(Value::as_str)
        })
        .map(str::trim)
        .filter(|description| !description.is_empty())
        .map(str::to_string)
}

/// Parse a tool call's `content` array into renderable pieces. Each item is
/// either a `diff` or a `content` block carrying text.
fn parse_tool_content(content: Option<&Value>) -> Vec<ToolContent> {
    let Some(arr) = content.and_then(Value::as_array) else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|item| match item.get("type").and_then(Value::as_str) {
            Some("diff") => Some(ToolContent::Diff {
                path: item
                    .get("path")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                old: item
                    .get("oldText")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                new: item
                    .get("newText")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            }),
            Some("terminal") => item
                .get("terminalId")
                .and_then(Value::as_str)
                .map(|id| ToolContent::Terminal(id.to_string())),
            _ => {
                // A `content` wrapper or a bare block: text renders as mono
                // output, an image block decodes to inline bytes shown in the
                // card rather than described; an
                // undecodable image degrades to a quiet text placeholder —
                // never a raw byte dump.
                let block = item.get("content").unwrap_or(item);
                match block.get("type").and_then(Value::as_str) {
                    Some("image") => Some(
                        block
                            .get("data")
                            .and_then(Value::as_str)
                            .and_then(base64_decode)
                            .map(|bytes| ToolContent::Image(Arc::new(bytes)))
                            .unwrap_or_else(|| ToolContent::Text("(image)".to_string())),
                    ),
                    _ => block_text(block)
                        .map(|s| ToolContent::Text(clip_tool_text(strip_wrapping_fence(&s)))),
                }
            }
        })
        .collect()
}

/// The most of one tool output a transcript keeps.
///
/// A step's output stays in memory for as long as the conversation is open,
/// and is written to its file: a read of a huge file or a command that prints
/// without end grew both with no bound. The card shows a few dozen lines; what
/// is past this is said rather than kept.
const TOOL_TEXT_MAX: usize = 1 << 20;

/// `text`, cut at [`TOOL_TEXT_MAX`] on a character boundary, saying how much
/// was left out.
fn clip_tool_text(mut text: String) -> String {
    if text.len() <= TOOL_TEXT_MAX {
        return text;
    }
    let mut end = TOOL_TEXT_MAX;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let cut = text.len() - end;
    text.truncate(end);
    text.push_str(&format!("\n… {} KB more not kept", cut.div_ceil(1024)));
    text
}

/// Unwrap a tool-output text that is exactly one markdown code fence. Claude's
/// `claude-code-acp` adapter wraps command output as "```console\n…\n```" (the
/// fence grows past 3 backticks when the output itself contains one) so
/// markdown-rendering clients get a code block — but the tool card's OUT
/// section renders raw mono text, where the fence marks would show literally.
/// Anything that isn't a single whole-text fence passes through untouched.
fn strip_wrapping_fence(text: &str) -> String {
    let lines: Vec<&str> = text.trim_end().lines().collect();
    if lines.len() < 2 {
        return text.to_string();
    }
    let ticks = lines[0].chars().take_while(|&c| c == '`').count();
    // Opening fence: ≥3 backticks + an optional info string (no backticks).
    if ticks < 3 || lines[0][ticks..].contains('`') {
        return text.to_string();
    }
    // Closing fence: backticks only, at least as many as the opener.
    let last = lines[lines.len() - 1].trim_end();
    if last.len() < ticks || !last.chars().all(|c| c == '`') {
        return text.to_string();
    }
    lines[1..lines.len() - 1].join("\n")
}

/// Standard-alphabet base64 decode (dependency-free, mirroring the encoder in
/// `client.rs`). `None` on any non-base64 input; padding optional.
pub fn base64_decode(s: &str) -> Option<Vec<u8>> {
    fn val(c: u8) -> Option<u32> {
        match c {
            b'A'..=b'Z' => Some((c - b'A') as u32),
            b'a'..=b'z' => Some((c - b'a') as u32 + 26),
            b'0'..=b'9' => Some((c - b'0') as u32 + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let bytes: Vec<u8> = s
        .bytes()
        .filter(|b| !b.is_ascii_whitespace() && *b != b'=')
        .collect();
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    for chunk in bytes.chunks(4) {
        if chunk.len() == 1 {
            return None; // a lone 6-bit group can't carry a byte
        }
        let mut acc: u32 = 0;
        for &b in chunk {
            acc = (acc << 6) | val(b)?;
        }
        acc <<= 6 * (4 - chunk.len()) as u32;
        let n = chunk.len() * 6 / 8;
        out.extend_from_slice(&acc.to_be_bytes()[1..1 + n]);
    }
    Some(out)
}

/// Pull the text out of a content field, which may be a single content block or
/// an array of them. Only `type: "text"` blocks contribute (Phase 2).
fn extract_text(content: Option<&Value>) -> Option<String> {
    let content = content?;
    if let Some(arr) = content.as_array() {
        let joined: String = arr.iter().filter_map(block_text).collect();
        (!joined.is_empty()).then_some(joined)
    } else {
        block_text(content)
    }
}

fn block_text(block: &Value) -> Option<String> {
    match block.get("type").and_then(Value::as_str) {
        Some("text") => block
            .get("text")
            .and_then(Value::as_str)
            .map(str::to_string),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The adapter sends it in the person's voice, as a user chunk.
    #[test]
    fn the_interrupted_marker_is_its_own_update() {
        let chunk = |text: &str| {
            json!({
                "sessionUpdate": "user_message_chunk",
                "content": { "type": "text", "text": text },
            })
        };
        assert_eq!(
            parse_session_update(&chunk("[Request interrupted by user for tool use]")),
            vec![Update::Interrupted]
        );
        assert_eq!(
            parse_session_update(&chunk("[Request interrupted by user]")),
            vec![Update::Interrupted]
        );
        assert_eq!(
            parse_session_update(&chunk("hi")),
            vec![Update::User("hi".into())]
        );
    }

    #[test]
    fn a_huge_tool_output_is_cut_and_says_so() {
        let text = "é".repeat(TOOL_TEXT_MAX);
        let kept = clip_tool_text(text);
        assert!(kept.len() < TOOL_TEXT_MAX + 64);
        assert!(kept.ends_with("… 1024 KB more not kept"));
        assert_eq!(clip_tool_text("ok".into()), "ok");
    }
    use serde_json::json;

    #[test]
    fn agent_message_chunk_single_block() {
        let u = json!({
            "sessionUpdate": "agent_message_chunk",
            "content": { "type": "text", "text": "Hello" }
        });
        assert_eq!(
            parse_session_update(&u),
            vec![Update::Agent("Hello".into())]
        );
    }

    #[test]
    fn agent_chunk_array_blocks_join() {
        let u = json!({
            "sessionUpdate": "agent_message_chunk",
            "content": [
                { "type": "text", "text": "Hello " },
                { "type": "image", "data": "…" },
                { "type": "text", "text": "world" }
            ]
        });
        assert_eq!(
            parse_session_update(&u),
            vec![Update::Agent("Hello world".into())]
        );
    }

    #[test]
    fn thought_and_user_chunks() {
        let t =
            json!({"sessionUpdate":"agent_thought_chunk","content":{"type":"text","text":"hmm"}});
        assert_eq!(
            parse_session_update(&t),
            vec![Update::Thought("hmm".into())]
        );
        let u = json!({"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"hi"}});
        assert_eq!(parse_session_update(&u), vec![Update::User("hi".into())]);
    }

    #[test]
    fn unknown_update_is_empty() {
        let u = json!({"sessionUpdate":"something_new","stuff":[]});
        assert!(parse_session_update(&u).is_empty());
    }

    #[test]
    fn plan_update_parses_entries() {
        let u = json!({
            "sessionUpdate": "plan",
            "entries": [
                { "content": "Read the config", "status": "completed", "priority": "medium" },
                { "content": "Fix the bug", "status": "in_progress" },
                { "content": "Add tests", "status": "pending" },
                { "status": "pending" } // no content → skipped
            ]
        });
        let Update::Plan(entries) = &parse_session_update(&u)[0] else {
            panic!()
        };
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].status, PlanStatus::Completed);
        assert_eq!(entries[1].status, PlanStatus::InProgress);
        assert_eq!(entries[2].content, "Add tests");
    }

    #[test]
    fn tool_image_content_decodes() {
        // "foobar" → Zm9vYmFy (the encoder's own test vector).
        let u = json!({
            "sessionUpdate": "tool_call",
            "toolCallId": "t1",
            "kind": "read",
            "content": [
                { "type": "content", "content": { "type": "image", "data": "Zm9vYmFy", "mimeType": "image/png" } },
                { "type": "content", "content": { "type": "image", "data": "!!not base64!!" } }
            ]
        });
        let Update::ToolCall(tc) = &parse_session_update(&u)[0] else {
            panic!()
        };
        assert_eq!(tc.content.len(), 2);
        let ToolContent::Image(bytes) = &tc.content[0] else {
            panic!("expected image")
        };
        assert_eq!(bytes.as_slice(), b"foobar");
        // Undecodable image degrades to a placeholder, never a byte dump.
        assert_eq!(tc.content[1], ToolContent::Text("(image)".into()));
    }

    #[test]
    fn base64_decode_roundtrips_the_encoder_vectors() {
        assert_eq!(base64_decode(""), Some(vec![]));
        assert_eq!(base64_decode("Zg=="), Some(b"f".to_vec()));
        assert_eq!(base64_decode("Zm8="), Some(b"fo".to_vec()));
        assert_eq!(base64_decode("Zm9v"), Some(b"foo".to_vec()));
        assert_eq!(base64_decode("Zm9vYmFy"), Some(b"foobar".to_vec()));
        // Unpadded and whitespace-broken input still decodes.
        assert_eq!(base64_decode("Zm9v\nYmFy"), Some(b"foobar".to_vec()));
        assert_eq!(base64_decode("Zg"), Some(b"f".to_vec()));
        // Garbage is rejected.
        assert_eq!(base64_decode("Z!"), None);
        assert_eq!(base64_decode("Z"), None);
    }

    #[test]
    fn tool_output_fence_unwraps() {
        // The adapter's "```console" wrapper comes off…
        assert_eq!(
            strip_wrapping_fence("```console\nls\nmain.rs\n```"),
            "ls\nmain.rs"
        );
        // …including the grown fence used when the output contains backticks…
        assert_eq!(
            strip_wrapping_fence("````\na ```fence``` inside\n````"),
            "a ```fence``` inside"
        );
        // …but anything that isn't one whole fenced block stays as-is.
        assert_eq!(strip_wrapping_fence("plain output"), "plain output");
        assert_eq!(
            strip_wrapping_fence("```console\nunterminated"),
            "```console\nunterminated"
        );
        assert_eq!(
            strip_wrapping_fence("```\nbody\n```\ntrailing"),
            "```\nbody\n```\ntrailing"
        );
        // Parse path applies the unwrap to tool-output text blocks.
        let u = json!({
            "sessionUpdate": "tool_call",
            "toolCallId": "t9",
            "title": "Run ls",
            "kind": "execute",
            "status": "completed",
            "content": [
                { "type": "content", "content": { "type": "text", "text": "```console\nok\n```" } }
            ]
        });
        let Update::ToolCall(tc) = &parse_session_update(&u)[0] else {
            panic!()
        };
        assert_eq!(tc.content[0], ToolContent::Text("ok".into()));
    }

    #[test]
    fn missing_text_is_empty() {
        let u = json!({"sessionUpdate":"agent_message_chunk","content":{"type":"image"}});
        assert!(parse_session_update(&u).is_empty());
    }

    #[test]
    fn tool_call_with_diff_and_output() {
        let u = json!({
            "sessionUpdate": "tool_call",
            "toolCallId": "t1",
            "title": "Edit main.rs",
            "kind": "edit",
            "status": "in_progress",
            "content": [
                { "type": "diff", "path": "src/main.rs", "oldText": "a", "newText": "b" },
                { "type": "content", "content": { "type": "text", "text": "ok" } }
            ]
        });
        let updates = parse_session_update(&u);
        assert_eq!(updates.len(), 1);
        let Update::ToolCall(tc) = &updates[0] else {
            panic!("expected tool call")
        };
        assert_eq!(tc.id, "t1");
        assert_eq!(tc.kind, ToolKind::Edit);
        assert_eq!(tc.status, ToolStatus::InProgress);
        assert_eq!(tc.content.len(), 2);
        assert_eq!(
            tc.content[0],
            ToolContent::Diff {
                path: "src/main.rs".into(),
                old: Some("a".into()),
                new: "b".into()
            }
        );
        assert_eq!(tc.content[1], ToolContent::Text("ok".into()));
    }

    #[test]
    fn tool_call_update_partial() {
        let u = json!({
            "sessionUpdate": "tool_call_update",
            "toolCallId": "t1",
            "status": "completed"
        });
        let Update::ToolUpdate(tu) = &parse_session_update(&u)[0] else {
            panic!("expected tool update")
        };
        assert_eq!(tu.id, "t1");
        assert_eq!(tu.status, Some(ToolStatus::Completed));
        assert_eq!(tu.title, None);
        assert_eq!(tu.description, None);
        assert_eq!(tu.content, None);
    }

    #[test]
    fn claude_bash_description_is_preserved_separately_from_the_command() {
        let u = json!({
            "sessionUpdate": "tool_call",
            "toolCallId": "bash-1",
            "title": "rg -n ManBtchNum src",
            "kind": "execute",
            "rawInput": {
                "command": "rg -n ManBtchNum src",
                "description": "Find references to ManBtchNum"
            },
            "_meta": {
                "claudeCode": {
                    "toolName": "Bash",
                    "title": "Search for ManBtchNum usage"
                }
            }
        });
        let Update::ToolCall(tc) = &parse_session_update(&u)[0] else {
            panic!("expected tool call")
        };
        assert_eq!(tc.title, "rg -n ManBtchNum src");
        assert_eq!(
            tc.description.as_deref(),
            Some("Search for ManBtchNum usage")
        );
    }

    #[test]
    fn unknown_kind_falls_back_to_other() {
        let u = json!({"sessionUpdate":"tool_call","toolCallId":"x","kind":"weird"});
        let Update::ToolCall(tc) = &parse_session_update(&u)[0] else {
            panic!()
        };
        assert_eq!(tc.kind, ToolKind::Other);
        assert_eq!(tc.status, ToolStatus::Pending);
    }

    #[test]
    fn available_commands_parse() {
        let u = json!({
            "sessionUpdate": "available_commands_update",
            "availableCommands": [
                { "name": "init", "description": "Initialize" },
                { "name": "review" }
            ]
        });
        let Update::Commands(cmds) = &parse_session_update(&u)[0] else {
            panic!()
        };
        assert_eq!(cmds.len(), 2);
        assert_eq!(cmds[0].name, "init");
        assert_eq!(cmds[1].description, "");
    }

    #[test]
    fn current_mode_update_parse() {
        let u = json!({ "sessionUpdate": "current_mode_update", "currentModeId": "plan" });
        assert_eq!(
            parse_session_update(&u),
            vec![Update::ModeChanged("plan".into())]
        );
    }

    #[test]
    fn config_options_parse_drops_mode_keeps_model() {
        let cfg = json!([
            {
                "id": "mode", "name": "Mode", "currentValue": "default", "type": "select",
                "options": [{ "name": "Default", "value": "default" }]
            },
            {
                "id": "model", "name": "Model", "currentValue": "default", "type": "select",
                "options": [
                    { "name": "Default (recommended)", "value": "default" },
                    { "name": "Opus", "value": "opus[1m]",
                      "description": "Opus 5 with 1M context" },
                    { "name": "Sonnet", "value": "sonnet", "description": "   " }
                ]
            }
        ]);
        let opts = parse_config_options(Some(&cfg));
        // The `mode` group is dropped; only `model` survives.
        assert_eq!(opts.len(), 1);
        let model = &opts[0];
        assert_eq!(model.id, "model");
        assert_eq!(model.current.as_deref(), Some("default"));
        assert_eq!(model.choices.len(), 3);
        assert_eq!(model.choices[1].value, "opus[1m]");
        assert_eq!(model.choices[1].name, "Opus");
        assert_eq!(
            model.choices[1].description.as_deref(),
            Some("Opus 5 with 1M context")
        );
        assert_eq!(
            model.choices[0].description, None,
            "the protocol does not promise one"
        );
        assert_eq!(
            model.choices[2].description, None,
            "blank is absent, or the row grows a second line saying nothing"
        );
    }

    #[test]
    fn config_option_update_parse() {
        let u = json!({
            "sessionUpdate": "config_option_update",
            "configOptions": [
                { "id": "model", "name": "Model", "currentValue": "sonnet", "type": "select",
                  "options": [{ "name": "Sonnet", "value": "sonnet" }] }
            ]
        });
        let Update::ConfigOptions(opts) = &parse_session_update(&u)[0] else {
            panic!()
        };
        assert_eq!(opts.len(), 1);
        assert_eq!(opts[0].current.as_deref(), Some("sonnet"));
    }

    fn parsed(v: Value) -> Vec<Update> {
        parse_session_update(&v)
    }

    fn usage(limit: Option<RateLimit>, cycle_end: bool) -> Update {
        Update::Usage(Usage {
            used: 84000,
            size: 200000,
            limit,
            cycle_end,
        })
    }

    #[test]
    fn usage_update_reads_used_and_size() {
        let u = json!({"sessionUpdate":"usage_update","used":84000,"size":200000});
        assert_eq!(parsed(u), vec![usage(None, false)]);
    }

    #[test]
    fn usage_update_reads_claude_rate_limit() {
        let u = json!({"sessionUpdate":"usage_update","used":84000,"size":200000,
            "_meta":{"_claude/rateLimit":{"status":"allowed_warning","utilization":0.78,
            "resetsAt":1793100000u64,"rateLimitType":"five_hour"}}});
        let limit = RateLimit {
            status: LimitStatus::Warning,
            utilization: Some(0.78),
            resets_at: Some(1793100000),
            kind: Some("five_hour".into()),
        };
        assert_eq!(parsed(u), vec![usage(Some(limit), false)]);
        let odd = json!({"sessionUpdate":"usage_update","used":84000,"size":200000,
            "_meta":{"_claude/rateLimit":{"status":"weird"}}});
        assert_eq!(parsed(odd), vec![usage(None, false)]);
    }

    #[test]
    fn usage_update_marks_a_cycle_end_by_origin() {
        let with = |origin: Value| {
            json!({"sessionUpdate":"usage_update","used":84000,"size":200000,
                "_meta":{"_claude/origin": origin}})
        };
        assert_eq!(
            parsed(with(json!({"kind":"task-notification"}))),
            vec![usage(None, true)]
        );
        assert_eq!(
            parsed(with(json!({"kind":"human"}))),
            vec![usage(None, false)]
        );
    }

    #[test]
    fn usage_update_without_numbers_is_dropped() {
        assert!(parsed(json!({"sessionUpdate":"usage_update","size":5})).is_empty());
        assert!(parsed(json!({"sessionUpdate":"usage_update","used":5})).is_empty());
    }

    #[test]
    fn async_task_spawned_reads_name_and_kind() {
        let spawned = |name: &str| {
            Update::Background(BackgroundReport::Spawned {
                id: "t1".into(),
                name: name.into(),
                kind: "shell".into(),
            })
        };
        let u = json!({"sessionUpdate":"async_task_spawned","asyncTaskId":"t1",
            "name":"npm run dev","taskType":"shell"});
        assert_eq!(parsed(u), vec![spawned("npm run dev")]);
        let u = json!({"sessionUpdate":"async_task_spawned","asyncTaskId":"t1",
            "description":"dev server","taskType":"shell"});
        assert_eq!(parsed(u), vec![spawned("dev server")]);
        let u = json!({"sessionUpdate":"async_task_spawned","asyncTaskId":"t1","taskType":"shell"});
        assert_eq!(parsed(u), vec![spawned("shell")]);
    }

    #[test]
    fn async_task_terminal_states_end_it() {
        let state = |s: &str| {
            parsed(json!({"sessionUpdate":"async_task_state_update","asyncTaskId":"t1","state":s}))
        };
        for s in ["completed", "failed", "stopped"] {
            assert_eq!(
                state(s),
                vec![Update::Background(BackgroundReport::Ended {
                    id: "t1".into()
                })]
            );
        }
        assert!(state("running").is_empty());
        assert!(state("mystery").is_empty());
    }

    #[test]
    fn async_task_progress_is_ignored() {
        let u = json!({"sessionUpdate":"async_task_progress","asyncTaskId":"t1"});
        assert!(parsed(u).is_empty());
    }
}
