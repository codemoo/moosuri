use super::*;
use serde_json::json;
fn wav(seconds: usize) -> Vec<u8> {
    let data = seconds * 16000 * 2;
    let mut b = Vec::new();
    b.extend(b"RIFF");
    b.extend(((36 + data) as u32).to_le_bytes());
    b.extend(b"WAVEfmt ");
    b.extend(16u32.to_le_bytes());
    b.extend(1u16.to_le_bytes());
    b.extend(1u16.to_le_bytes());
    b.extend(16000u32.to_le_bytes());
    b.extend(32000u32.to_le_bytes());
    b.extend(2u16.to_le_bytes());
    b.extend(16u16.to_le_bytes());
    b.extend(b"data");
    b.extend((data as u32).to_le_bytes());
    b.resize(44 + data, 0);
    b
}
#[test]
fn long_canonical_audio_preserves_every_sample_and_rejects_malformed_envelopes() {
    let raw = wav(121);
    let chunks = wav::chunks(&raw).unwrap();
    assert_eq!(
        chunks.iter().map(|c| c.duration_ms).collect::<Vec<_>>(),
        [120000, 1000]
    );
    let data = chunks
        .iter()
        .flat_map(|c| c.bytes[44..].iter().copied())
        .collect::<Vec<_>>();
    assert_eq!(data, raw[44..]);
    for offset in [0, 4, 8, 16, 20, 22, 24, 28, 32, 34, 36, 40] {
        let mut bad = raw.clone();
        bad[offset] ^= 1;
        assert!(wav::chunks(&bad).is_err(), "{offset}");
    }
    assert!(wav::chunks(&wav(1201)).is_err());
}
#[test]
fn normalized_transcript_has_exact_raw_hash_stable_identity_and_explicit_attribution() {
    let id = uuid::Uuid::parse_str("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa").unwrap();
    let value = json!({"format_version":1,"kind":"moosuri_call_export","call_id":id,"ended_at":100,"settled":true,"title":"Synthetic call","audio":null,"transcript":{"segments":[{"speaker":"Synthetic Mina","text":"I prefer quiet meetings."}]}});
    let raw = serde_json::to_vec(&value).unwrap();
    let parsed = format::parse(&raw, 1000).unwrap();
    let (source, metadata) = format::source(
        &parsed,
        "EXACT_RAW_HASH",
        parsed.transcript.as_ref().unwrap(),
        None,
    )
    .unwrap();
    assert_eq!(source.speaker.as_deref(), Some("Synthetic Mina"));
    assert!(source.text.contains("Exact export SHA256: EXACT_RAW_HASH"));
    assert_eq!(metadata.external_id, source.external_id);
    let mut alternate = value.clone();
    alternate["call_id"] = json!(id.to_string().to_uppercase());
    let alternate = format::parse(&serde_json::to_vec(&alternate).unwrap(), 1000).unwrap();
    let (same_id, _) = format::source(
        &alternate,
        "ALTERNATE_RAW_HASH",
        alternate.transcript.as_ref().unwrap(),
        None,
    )
    .unwrap();
    assert_eq!(source.external_id, same_id.external_id);
    let mut changed = value.clone();
    changed["transcript"]["segments"][0]["speaker"] = serde_json::Value::Null;
    let changed = format::parse(&serde_json::to_vec(&changed).unwrap(), 1000).unwrap();
    let (unknown, _) = format::source(
        &changed,
        "NEW_RAW_HASH",
        changed.transcript.as_ref().unwrap(),
        None,
    )
    .unwrap();
    assert_eq!(source.external_id, unknown.external_id);
    assert_ne!(source.version, unknown.version);
    assert_eq!(
        unknown.speaker.as_deref(),
        Some(moosuri_core::UNATTRIBUTED_CALL_SPEAKER)
    );
    let mut invalid = value;
    invalid["transcript"]["segments"][0]["speaker"] =
        json!(moosuri_core::UNATTRIBUTED_CALL_SPEAKER);
    assert!(format::parse(&serde_json::to_vec(&invalid).unwrap(), 1000).is_err());
}
