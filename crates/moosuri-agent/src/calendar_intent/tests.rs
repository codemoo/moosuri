use super::*;
use serde_json::{Value, json};
#[test]
fn calendar_current_schema_has_finite_codes_required_nullable_fields_and_no_normalization() {
    let definition = definition();
    let p = &definition.parameters["properties"];
    let mut required = definition.parameters["required"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect::<Vec<_>>();
    required.sort();
    let mut fields = moosuri_core::CALENDAR_CHAT_FIELDS.to_vec();
    fields.sort();
    assert_eq!(required, fields);
    for (key, codes) in [
        ("operation", moosuri_core::CALENDAR_CHAT_OPERATIONS),
        ("clarification", moosuri_core::CALENDAR_CHAT_CLARIFICATIONS),
    ] {
        let expected = codes
            .iter()
            .map(|v| json!(v))
            .chain(std::iter::once(Value::Null))
            .collect::<Vec<_>>();
        assert_eq!(p[key]["enum"], json!(expected));
    }
    let mut value = json!({});
    for field in moosuri_core::CALENDAR_CHAT_FIELDS {
        value[*field] = Value::Null;
    }
    value["domain"] = json!("calendar");
    value["decision"] = json!("clarify");
    value["clarification"] = json!("time");
    let text = value.to_string();
    assert!(parse(&text).is_some());
    // Original serde decoding must retain duplicate-key rejection.
    assert!(parse(&format!("{{\"decision\":\"command\",{}", &text[1..])).is_none());
    value["operation"] = json!("move");
    assert!(parse(&value.to_string()).is_none());
    value["operation"] = Value::Null;
    value["clarification"] = json!("Please supply a time");
    assert!(parse(&value.to_string()).is_none());
    value["clarification"] = Value::Null;
    assert!(parse(&value.to_string()).is_none());
    value["decision"] = json!("unsupported");
    assert!(parse(&value.to_string()).unwrap().clarification.is_none());
    // Legacy persisted DTO omissions still decode; current output must be complete.
    value.as_object_mut().unwrap().remove("move_date");
    assert!(serde_json::from_value::<moosuri_core::CalendarChatDecision>(value.clone()).is_ok());
    assert!(parse(&value.to_string()).is_none());
}
