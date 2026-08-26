//! Translate ACP `session/update` notifications into a flat list of updates
//!. Pure — operates on `serde_json::Value` so it is
//! unit-testable without the transport. Phase 2 handles the message/thought/user
//! content streams; tool calls, plans, commands and selectors arrive in Phase 3.

use crate::acp::types::{
    ConfigChoice, ConfigOption, PlanEntry, PlanStatus, SlashCommand, ToolCall, ToolCallUpdate,
    ToolContent, ToolKind, ToolStatus,
};
use serde_json::Value;
use std::sync::Arc;

/// A single parsed effect of a `session/update`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Update {
    /// A chunk of the agent's reply text (markdown).
    Agent(String),
    /// A chunk of the agent's reasoning.
    Thought(String),
    /// A chunk echoed from the user (resume replay).
    User(String),
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
}

/// Parse the inner `update` object of a `session/update` notification.
pub fn parse_session_update(update: &Value) -> Vec<Update> {
    let kind = update
        .get("sessionUpdate")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let text = || extract_text(update.get("content"));
    match kind {
        "agent_message_chunk" => text().map(Update::Agent).into_iter().collect(),
        "agent_thought_chunk" => text().map(Update::Thought).into_iter().collect(),
        "user_message_chunk" => text().map(Update::User).into_iter().collect(),
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
        _ => Vec::new(),
    }
}

/// Parse a `configOptions` array (from `session/new` or `config_option_update`)
/// into the selectable groups. The `"mode"` group is dropped — session mode is
/// driven by the standard `modes` field + `session/set_mode`.
pub fn parse_config_options(options: Option<&Value>) -> Vec<ConfigOption> {
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
                    Some(ConfigChoice { value, name })
                })
                .collect()
        })
        .unwrap_or_default();
    Some(ConfigOption {
        id,
        name,
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
                    _ => block_text(block).map(|s| ToolContent::Text(strip_wrapping_fence(&s))),
                }
            }
        })
        .collect()
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
                    { "name": "Opus", "value": "opus[1m]" },
                    { "name": "Sonnet", "value": "sonnet" }
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
}
