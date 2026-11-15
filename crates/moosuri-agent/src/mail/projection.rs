//! Model-only projection. Bound the entire escaped result, not independent text
//! fields; exact provider identifiers remain intact in any exposed metadata.
use super::*;
use serde_json::{Value, json};
const LIMIT: usize = 12 * 1024;
fn size(value: &Value) -> usize {
    serde_json::to_vec(value).map_or(usize::MAX, |v| v.len())
}
fn prefix(value: &str, limit: usize) -> String {
    let mut end = value.len().min(limit);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}
fn sync_attachments(message: &mut Value, total: usize) {
    let exposed = message["attachments"].as_array().map_or(0, Vec::len);
    let names = message["attachments"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|a| a.get("filename").cloned())
        .collect::<Vec<_>>();
    message["mail"]["attachment_names"] = json!(names);
    message["attachment_total"] = json!(total);
    message["attachments_next_start_index"] = if exposed < total {
        json!(exposed)
    } else {
        Value::Null
    };
    message["mail"]["attachment_names_truncated"] = json!(exposed < total);
    if exposed < total {
        message["partial"] = true.into();
    }
}
fn shorten_text(message: &mut Value, max: usize) -> bool {
    let Some(text) = message.pointer("/mail/text").and_then(Value::as_str) else {
        return false;
    };
    if text.len() <= max {
        return false;
    }
    message["mail"]["text"] = prefix(text, max).into();
    message["mail"]["text_truncated"] = true.into();
    message["partial"] = true.into();
    true
}
fn pop(message: &mut Value, field: &str, keep: usize, flag: &str) -> bool {
    let Some(values) = message.pointer_mut(field).and_then(Value::as_array_mut) else {
        return false;
    };
    if values.len() <= keep {
        return false;
    }
    values.pop();
    if flag == "headers" {
        message["mail"]["headers_truncated"] = true.into();
    } else {
        message[flag] = true.into();
    }
    message["partial"] = true.into();
    true
}
fn message(message: &mut Value, budget: usize) {
    let total = message["attachments"].as_array().map_or(0, Vec::len);
    if let Some(attachments) = message["attachments"].as_array_mut() {
        attachments.truncate(4);
    }
    sync_attachments(message, total);
    shorten_text(message, 4096);
    while size(message) + 64 > budget {
        // Preserve a useful body and at least one attachment when possible.
        if pop(message, "/attachments", 1, "partial") {
            sync_attachments(message, total);
            continue;
        }
        let text_len = message["mail"]["text"].as_str().map_or(0, str::len);
        if text_len > 256 {
            shorten_text(message, (text_len / 2).max(256));
            continue;
        }
        if ["/mail/bcc", "/mail/cc", "/mail/to", "/mail/from"]
            .iter()
            .any(|field| pop(message, field, 1, "headers"))
        {
            continue;
        }
        if pop(message, "/labels", 4, "labels_truncated") {
            continue;
        }
        if pop(message, "/attachments", 0, "partial") {
            sync_attachments(message, total);
            continue;
        }
        if text_len > 0 {
            shorten_text(message, text_len / 2);
            continue;
        }
        if let Some(subject) = message["mail"]["subject"].as_str()
            && !subject.is_empty()
        {
            message["mail"]["subject"] = prefix(subject, subject.len() / 2).into();
            message["mail"]["headers_truncated"] = true.into();
            continue;
        }
        if ["/mail/bcc", "/mail/cc", "/mail/to", "/mail/from"]
            .iter()
            .any(|field| pop(message, field, 0, "headers"))
        {
            continue;
        }
        if pop(message, "/labels", 0, "labels_truncated") {
            continue;
        }
        if !message["mail"]["message_id"].is_null() {
            message["mail"]["message_id"] = Value::Null;
            message["mail"]["headers_truncated"] = true.into();
            continue;
        }
        break; // Only fixed fields plus exact <=256-byte message/thread IDs remain.
    }
}
pub(super) fn bounded(mut value: Value) -> KnowledgeToolResult {
    if value.get("mail").is_some() {
        message(&mut value, LIMIT - 128);
    } else if let Some(messages) = value.get_mut("messages").and_then(Value::as_array_mut)
        && messages.iter().any(|v| v.get("mail").is_some())
    {
        let truncated = messages.len() > 3;
        if truncated {
            messages.drain(..messages.len() - 3);
        }
        let budget = (LIMIT - 1024) / messages.len().max(1);
        for item in messages.iter_mut() {
            message(item, budget);
        }
        let partial = truncated || messages.iter().any(|v| v["partial"] == true);
        if partial {
            value["partial"] = true.into();
        }
        if truncated {
            value["truncated"] = true.into();
        }
    }
    let result = KnowledgeToolResult::External(value);
    // Search, metadata pages and attachment pages are separately bounded too.
    if serde_json::to_vec(&result).is_ok_and(|v| v.len() <= LIMIT) {
        result
    } else {
        KnowledgeToolResult::Error(ToolReadError::TooLargeNarrowRead)
    }
}
