use super::*;
use base64::engine::general_purpose::STANDARD;
use moosuri_core::{ModelSelection, ModelSource};
pub(super) async fn response(s: &Provider, body: &Value) -> axum::response::Response {
    s.models.fetch_add(1, Ordering::SeqCst);
    s.requests.lock().unwrap().push(body.clone());
    assert_eq!(body["store"], false);
    assert_eq!(body["stream"], true);
    assert!(body.get("tools").is_none());
    let content = body["input"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|m| {
            m["content"]
                .as_array()
                .filter(|c| c.iter().any(|v| v["type"] == "input_image"))
        })
        .unwrap();
    assert_eq!(content[0]["type"], "input_text");
    assert_eq!(content[1]["type"], "input_image");
    let encoded = content[1]["image_url"]
        .as_str()
        .unwrap()
        .strip_prefix("data:image/jpeg;base64,")
        .unwrap();
    let bytes = STANDARD.decode(encoded).unwrap();
    let image = image::load_from_memory(&bytes).unwrap().to_rgb8();
    let answer: String = (0..16)
        .map(|n| {
            let p = image.get_pixel((n % 4) * 32 + 16, (n / 4) * 32 + 16).0;
            if p[0] > 150 && p[1] > 150 {
                'Y'
            } else if p[0] > p[1] && p[0] > p[2] {
                'R'
            } else if p[1] > p[2] {
                'G'
            } else {
                'B'
            }
        })
        .collect();
    let answer = if s.answer_phase.load(Ordering::SeqCst) == 1 {
        "WRONG".to_owned()
    } else {
        answer
    };
    if s.hold_tool.load(Ordering::SeqCst) == 1 {
        s.tool_entered.notify_one();
        s.tool_release.notified().await;
    }
    let item = json!({"type":"message","id":"vision-message","role":"assistant","status":"completed","content":[{"type":"output_text","text":answer,"annotations":[]}]});
    let events = [
        json!({"type":"response.created","response":{"id":"vision-response","model":"fixture-model"}}),
        json!({"type":"response.output_item.added","output_index":0,"item":{"type":"message","id":"vision-message","role":"assistant","content":[]}}),
        json!({"type":"response.content_part.added","item_id":"vision-message","output_index":0,"content_index":0,"part":{"type":"output_text","text":""}}),
        json!({"type":"response.output_text.delta","item_id":"vision-message","output_index":0,"content_index":0,"delta":answer}),
        json!({"type":"response.output_text.done","item_id":"vision-message","output_index":0,"content_index":0,"text":answer}),
        json!({"type":"response.content_part.done","item_id":"vision-message","output_index":0,"content_index":0,"part":{"type":"output_text","text":answer,"annotations":[]}}),
        json!({"type":"response.output_item.done","output_index":0,"item":item}),
        json!({"type":"response.completed","response":{"id":"vision-response","model":"fixture-model","status":"completed","output":[item],"usage":{"input_tokens":8,"output_tokens":16}}}),
    ];
    let text = events
        .iter()
        .map(|e| format!("data: {e}\n\n"))
        .collect::<String>();
    (
        [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
        text,
    )
        .into_response()
}
#[tokio::test]
async fn siwc_image_proof_reads_actual_random_pixels_and_rejects_late_owner_revocation() {
    let _g = crate::tests::TEST_LOCK.lock().await;
    let _d = tempfile::tempdir().unwrap();
    let store = Store::open(_d.path()).unwrap();
    store
        .bootstrap(
            "Synthetic image owner".into(),
            "image-owner@example.test".into(),
            PASSWORD.into(),
            false,
        )
        .await
        .unwrap();
    let u = store
        .login("image-owner@example.test".into(), PASSWORD.into())
        .await
        .unwrap()
        .user;
    let f = fixture().await;
    let svc = service(store.clone(), &f);
    let launch = login(&svc, &u).await;
    proof(&svc, &u, launch.binding_id.clone(), launch.revision).await;
    let selection = ModelSelection {
        source: ModelSource::PersonalChatGpt,
        provider_id: launch.binding_id.clone(),
        provider_revision: launch.revision,
        model: "fixture-model".into(),
        effort: None,
    };
    f.state.tool_mode.store(90, Ordering::SeqCst);
    f.state.answer_phase.store(1, Ordering::SeqCst);
    assert!(
        svc.personal_vision_probe(&u, selection.clone())
            .await
            .is_err()
    );
    assert!(!store.personal_vision_proven(&u, &selection).await.unwrap());
    f.state.answer_phase.store(0, Ordering::SeqCst);
    svc.personal_vision_probe(&u, selection.clone())
        .await
        .unwrap();
    assert!(store.personal_vision_proven(&u, &selection).await.unwrap());
    f.state.hold_tool.store(1, Ordering::SeqCst);
    let verify = svc.personal_vision_probe(&u, selection.clone());
    let revoke = async {
        f.state.tool_entered.notified().await;
        store.logout(&u).await.unwrap();
        f.state.tool_release.notify_one();
    };
    let (result, ()) = tokio::join!(verify, revoke);
    assert!(result.is_err());
    assert_eq!(f.state.requests.lock().unwrap().len(), 3);
}
