//! The agent's parked asks as the client reads them: a permission request
//! and a form, and the answer a form is sent back with.

use crate::acp::types::{
    ElicitChoice, ElicitField, ElicitKind, ElicitOutcome, ElicitValue, Elicitation,
    PermissionOption, PermissionRequest, ToolKind,
};
use serde_json::{json, Value};

/// Parse a `session/request_permission` into a [`PermissionRequest`], carrying
/// the raw JSON-RPC `id` so the eventual response echoes it.
pub(super) fn parse_permission(rpc_id: Value, params: &Value) -> PermissionRequest {
    let tool_call = params.get("toolCall");
    let tool_call_id = tool_call
        .and_then(|t| t.get("toolCallId"))
        .and_then(Value::as_str)
        .map(String::from);
    let kind = tool_call
        .and_then(|t| t.get("kind"))
        .and_then(Value::as_str)
        .map(ToolKind::parse)
        .unwrap_or(ToolKind::Other);
    let title = tool_call
        .and_then(|t| t.get("title"))
        .and_then(Value::as_str)
        .unwrap_or("Permission requested")
        .to_string();
    let options = params
        .get("options")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .map(|o| PermissionOption {
                    id: o
                        .get("optionId")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    name: o
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("Option")
                        .to_string(),
                    kind: o
                        .get("kind")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                })
                .collect()
        })
        .unwrap_or_default();
    PermissionRequest {
        rpc_id,
        tool_call_id,
        kind,
        title,
        options,
        child: None,
    }
}

// ── Elicitation (`elicitation/create`) ───────────────────────────────────────

/// Suffix marking a select field's paired free-text "Other" property. Claude's
/// adapter emits one per question (`question_0` + `question_0_custom`).
pub(super) const CUSTOM_SUFFIX: &str = "_custom";

/// Parse an `elicitation/create` into an [`Elicitation`]. `None` for anything we
/// can't render as a form — url mode, a missing/empty `requestedSchema` — so the
/// caller declines instead of parking it.
pub(super) fn parse_elicitation(rpc_id: Value, params: &Value) -> Option<Elicitation> {
    if params.get("mode").and_then(Value::as_str) == Some("url") {
        return None;
    }
    let props = params
        .get("requestedSchema")
        .and_then(|s| s.get("properties"))
        .and_then(Value::as_object)?;

    // `serde_json` maps are sorted, not insertion-ordered, so `question_10`
    // would sort before `question_2` and a question's `_custom` box between
    // them. Order by the numeric suffix where the adapter's convention holds,
    // and leave any other schema (an MCP form) in plain key order after it.
    let mut keys: Vec<&String> = props.keys().collect();
    keys.sort_by_key(|k| field_order(k));

    // A question's "Other" box is a property of its own that belongs to the
    // question rather than standing beside it: Claude's adapter names it
    // `<key>_custom`, Codex's says so in its `_meta`.
    let other_box = |key: &str| -> Option<String> {
        let custom = format!("{key}{CUSTOM_SUFFIX}");
        if props.contains_key(&custom) {
            return Some(custom);
        }
        props
            .iter()
            .find(|(_, schema)| {
                let codex = schema.pointer("/_meta/codex");
                codex
                    .and_then(|c| c.get("questionId"))
                    .and_then(Value::as_str)
                    == Some(key)
                    && codex.and_then(|c| c.get("role")).and_then(Value::as_str)
                        == Some("user_note")
            })
            .map(|(note, _)| note.clone())
    };
    let boxes: Vec<String> = keys.iter().filter_map(|k| other_box(k)).collect();
    let fields = keys
        .iter()
        .filter(|k| !boxes.contains(k))
        .map(|k| {
            let schema = &props[*k];
            ElicitField {
                key: (*k).clone(),
                title: str_field(schema, "title"),
                description: str_field(schema, "description"),
                kind: parse_elicit_kind(schema),
                custom_key: other_box(k),
            }
        })
        .collect::<Vec<_>>();
    if fields.is_empty() {
        return None;
    }

    Some(Elicitation {
        rpc_id,
        tool_call_id: str_field(params, "toolCallId"),
        message: str_field(params, "message").unwrap_or_else(|| "The agent is asking".into()),
        fields,
        child: None,
    })
}

/// Sort key placing `question_<n>` (and its `_custom` box) in the adapter's own
/// order; everything else sorts after, by key.
pub(super) fn field_order(key: &str) -> (u8, u64, u8, String) {
    let (stem, custom) = match key.strip_suffix(CUSTOM_SUFFIX) {
        Some(stem) => (stem, 1),
        None => (key, 0),
    };
    match stem
        .strip_prefix("question_")
        .and_then(|n| n.parse::<u64>().ok())
    {
        Some(n) => (0, n, custom, String::new()),
        None => (1, 0, custom, key.to_string()),
    }
}

/// A string property of a JSON object, if present and non-empty.
pub(super) fn str_field(v: &Value, key: &str) -> Option<String> {
    v.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(String::from)
}

/// Classify one schema property: an array of choices → multi-select, a scalar
/// with choices → single select, anything else → free text.
pub(super) fn parse_elicit_kind(schema: &Value) -> ElicitKind {
    if schema.get("type").and_then(Value::as_str) == Some("array") {
        return match schema.get("items").map(parse_choices) {
            Some(c) if !c.is_empty() => ElicitKind::MultiSelect(c),
            _ => ElicitKind::Text,
        };
    }
    match parse_choices(schema) {
        c if c.is_empty() => ElicitKind::Text,
        c => ElicitKind::Select(c),
    }
}

/// The choices of an enum-typed schema: ACP's titled `oneOf`/`anyOf` option
/// objects, or a plain JSON-Schema `enum` array (optionally labelled by the
/// MCP-style `enumNames`).
pub(super) fn parse_choices(schema: &Value) -> Vec<ElicitChoice> {
    if let Some(arr) = schema
        .get("oneOf")
        .or_else(|| schema.get("anyOf"))
        .and_then(Value::as_array)
    {
        return arr
            .iter()
            .filter_map(|o| {
                let value = o.get("const").and_then(Value::as_str)?.to_string();
                let label = str_field(o, "title").unwrap_or_else(|| value.clone());
                Some(ElicitChoice {
                    value,
                    label,
                    description: str_field(o, "description"),
                })
            })
            .collect();
    }
    let names = schema.get("enumNames").and_then(Value::as_array);
    schema
        .get("enum")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .enumerate()
                .filter_map(|(i, v)| {
                    let value = v.as_str()?.to_string();
                    let label = names
                        .and_then(|n| n.get(i))
                        .and_then(Value::as_str)
                        .map(String::from)
                        .unwrap_or_else(|| value.clone());
                    Some(ElicitChoice {
                        value,
                        label,
                        description: None,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Build the `elicitation/create` result from the user's outcome.
pub(super) fn elicit_result(outcome: ElicitOutcome) -> Value {
    match outcome {
        ElicitOutcome::Accept(answers) => {
            let content: serde_json::Map<String, Value> = answers
                .into_iter()
                .map(|(k, v)| {
                    let v = match v {
                        ElicitValue::Text(s) => Value::String(s),
                        ElicitValue::List(l) => {
                            Value::Array(l.into_iter().map(Value::String).collect())
                        }
                    };
                    (k, v)
                })
                .collect();
            json!({ "action": "accept", "content": content })
        }
        ElicitOutcome::Decline => json!({ "action": "decline" }),
        ElicitOutcome::Cancel => json!({ "action": "cancel" }),
    }
}
