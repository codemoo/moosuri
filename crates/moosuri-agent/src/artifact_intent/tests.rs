use super::*;
#[test]
fn artifact_model_contract_requires_explicit_nulls_and_closed_current_decisions() {
    let mut value = json!({"decision":"command","action":"create","kind":"document","intent_quote":"save a document","target_quote":null});
    assert!(parse(&value.to_string()).is_some());
    let encoded = value.to_string();
    assert!(parse(&format!("{{\"decision\":\"clarify\",{}", &encoded[1..])).is_none());
    for field in moosuri_core::ARTIFACT_INTENT_FIELDS {
        let mut missing = value.clone();
        missing.as_object_mut().unwrap().remove(*field);
        assert!(parse(&missing.to_string()).is_none(), "missing {field}");
    }
    value["decision"] = json!("no_command");
    assert!(parse(&value.to_string()).is_none());
    value = json!({"decision":"clarify","action":null,"kind":null,"intent_quote":null,"target_quote":null});
    assert!(parse(&value.to_string()).is_some());
    value["decision"] = json!("Please specify a document");
    assert!(parse(&value.to_string()).is_none());
    let definition = definition();
    assert!(crate::intent_contract::exact_fields(
        &definition.parameters["properties"],
        moosuri_core::ARTIFACT_INTENT_FIELDS
    ));
    assert_eq!(
        definition.parameters["properties"]["action"],
        crate::intent_contract::nullable_enum(moosuri_core::ARTIFACT_INTENT_ACTIONS)
    );
    assert_eq!(
        definition.parameters["properties"]["kind"],
        crate::intent_contract::nullable_enum(ArtifactKind::ALL)
    );
}
