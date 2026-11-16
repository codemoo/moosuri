//! Schema mechanics only. No values are inferred, trimmed or normalized.
use serde::Serialize;
use serde_json::{Value, json};
pub(crate) fn nullable_enum(values: impl Serialize) -> Value {
    let mut values = serde_json::to_value(values)
        .expect("static vocabulary")
        .as_array()
        .expect("static vocabulary list")
        .clone();
    values.push(Value::Null);
    json!({"type":["string","null"],"enum":values})
}
pub(crate) fn exact_fields(value: &Value, fields: &[&str]) -> bool {
    value.as_object().is_some_and(|v| {
        v.len() == fields.len() && fields.iter().all(|field| v.contains_key(*field))
    })
}
