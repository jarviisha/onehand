//! JSON-RPC as this client writes it, and the prompt blocks a turn sends.

use super::{Outgoing, PROTOCOL_VERSION};
use crate::acp::types::{AcpEvent, Attachment, Mode};
use crate::attachment::{inline_image_mime, MAX_INLINE_IMAGE_BYTES};
use serde_json::{json, Value};
use tokio::io::AsyncWriteExt;

// ── JSON-RPC helpers ─────────────────────────────────────────────────────────

pub(super) fn alloc(next: &mut u64) -> u64 {
    let id = *next;
    *next += 1;
    id
}

pub(super) fn init_params() -> Value {
    json!({
        "protocolVersion": PROTOCOL_VERSION,
        "clientCapabilities": {
            "fs": { "readTextFile": true, "writeTextFile": true },
            "terminal": true,
            // Form elicitation (`elicitation/create`) is what unlocks the agent's
            // *question* prompts: Claude's adapter puts `AskUserQuestion` in
            // `disallowedTools` unless this is advertised, so without it the model
            // can never offer the user a choice — it just guesses and proceeds.
            // `{}` is the whole capability ("we can render a form"); we answer the
            // request in `handle_message` and, for a form we can't render, decline
            // immediately rather than park it forever.
            "elicitation": { "form": {} }
        }
    })
}

pub(super) fn request(id: u64, method: &str, params: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })
}

pub(super) fn notification(method: &str, params: Value) -> Value {
    json!({ "jsonrpc": "2.0", "method": method, "params": params })
}

pub(super) fn response_ok(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

/// A JSON-RPC error as a person reads it: its message, and what the agent
/// attached as `data`, as machine text. Its code is for programs and is not
/// said. An error with no message has nothing better to say than itself.
pub(super) fn in_words(err: &Value) -> (String, Option<String>) {
    let message = err
        .get("message")
        .and_then(Value::as_str)
        .map_or_else(|| err.to_string(), str::to_string);
    let detail = err.get("data").filter(|d| !d.is_null()).map(|d| match d {
        Value::String(s) => s.clone(),
        other => serde_json::to_string_pretty(other).unwrap_or_else(|_| other.to_string()),
    });
    (message, detail)
}

/// Whether an error is ACP's `auth_required`: the agent is not logged in. One
/// adapter answers `session/new` with it, another only the first prompt.
pub(super) fn login_required(err: &Value) -> bool {
    err.get("code").and_then(Value::as_i64) == Some(-32000)
}

pub(super) fn method_not_found(id: Value, method: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": -32601, "message": format!("method not found: {method}") }
    })
}

pub(super) fn response_err(id: Value, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32603, "message": message } })
}

pub(super) async fn write_msg(stdin: &mut Outgoing, msg: &Value) -> Result<(), String> {
    let mut line = serde_json::to_string(msg).map_err(|e| e.to_string())?;
    line.push('\n');
    stdin
        .write_all(line.as_bytes())
        .await
        .map_err(|e| e.to_string())?;
    stdin.flush().await.map_err(|e| e.to_string())?;
    Ok(())
}

/// Parse the `modes` object from a `session/new` result into a `Modes` event.
pub(super) fn parse_modes(modes: &Value) -> AcpEvent {
    let current = modes
        .get("currentModeId")
        .and_then(Value::as_str)
        .map(String::from);
    let available = modes
        .get("availableModes")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|m| {
                    let id = m
                        .get("id")
                        .or_else(|| m.get("modeId"))
                        .and_then(Value::as_str)?
                        .to_string();
                    let name = m
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or(&id)
                        .to_string();
                    Some(Mode { id, name })
                })
                .collect()
        })
        .unwrap_or_default();
    AcpEvent::Modes { current, available }
}

/// Build the ACP `prompt` content-block array: a text block plus one block per
/// attachment — inline `image` (base64) for images, else a `resource_link`.
/// Max image size sent inline (base64 in the prompt JSON). Reading is
/// whole-file-into-memory and base64 inflates it ~4/3× (plus a serialize copy),
/// so an uncapped read of a huge file would transiently allocate several times
/// its size. Anything larger degrades to a `resource_link`.
pub(super) async fn build_prompt_blocks(
    text: &str,
    attachments: &[Attachment],
    images: bool,
) -> Value {
    let mut blocks = vec![json!({ "type": "text", "text": text })];
    for att in attachments {
        let name = att
            .path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        if let Some(mime) = inline_image_mime(&att.path).filter(|_| images) {
            let size_ok = tokio::fs::metadata(&att.path)
                .await
                .map(|m| m.len() <= MAX_INLINE_IMAGE_BYTES)
                .unwrap_or(false);
            if size_ok {
                if let Ok(bytes) = tokio::fs::read(&att.path).await {
                    blocks.push(json!({
                        "type": "image",
                        "mimeType": mime,
                        "data": base64_encode(&bytes),
                    }));
                    continue;
                }
            }
        }
        blocks.push(json!({
            "type": "resource_link",
            "uri": file_uri(&att.path),
            "name": name,
        }));
    }
    Value::Array(blocks)
}

/// The part of `content` a read asked for: from `line` (counted from 1), at
/// most `limit` lines, each kept with its own line ending.
pub(super) fn lines_of(content: &str, line: Option<u64>, limit: Option<u64>) -> String {
    if line.is_none() && limit.is_none() {
        return content.to_string();
    }
    let skip = line.unwrap_or(1).saturating_sub(1) as usize;
    let take = limit.map_or(usize::MAX, |n| n as usize);
    content
        .split_inclusive('\n')
        .skip(skip)
        .take(take)
        .collect()
}

/// A `file://` URI with the path percent-encoded (RFC 3986): unreserved chars
/// and `/` pass through, everything else — spaces, `#`, `?`, non-ASCII — is
/// escaped byte-wise. A raw `path.display()` URI truncates at the first `#`
/// on the adapter side.
pub(super) fn file_uri(path: &std::path::Path) -> String {
    let mut out = String::from("file://");
    for &b in path.display().to_string().as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Dependency-free standard base64 encoder.
pub fn base64_encode(bytes: &[u8]) -> String {
    pub(super) const ALPHABET: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | (b[2] as u32);
        out.push(ALPHABET[(n >> 18 & 63) as usize] as char);
        out.push(ALPHABET[(n >> 12 & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(n >> 6 & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}
