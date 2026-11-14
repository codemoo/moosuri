//! Repository-owned closed export format. No vendor log format is inferred.
use super::*;
use moosuri_core::{SourceInput, SourceObjectMetadata, UNATTRIBUTED_CALL_SPEAKER};
use std::path::{Component, Path};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Export {
    pub format_version: u32,
    pub kind: String,
    pub call_id: String,
    pub ended_at: i64,
    pub settled: bool,
    pub title: String,
    pub audio: Option<Audio>,
    pub transcript: Option<Transcript>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Audio {
    pub file: String,
    pub sha256: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Transcript {
    pub segments: Vec<Segment>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Segment {
    pub speaker: Option<String>,
    pub text: String,
}
fn label(s: &str) -> bool {
    !s.trim().is_empty() && s.len() <= 160 && !s.chars().any(|c| c.is_control())
}
pub(crate) fn parse(bytes: &[u8], time: i64) -> work_sessions::Result<Export> {
    let mut e: Export = serde_json::from_slice(bytes).map_err(|_| "unsupported_call_export")?;
    if e.format_version != 1
        || e.kind != "moosuri_call_export"
        || uuid::Uuid::parse_str(&e.call_id).is_err()
        || !label(&e.title)
        || e.ended_at <= 0
        || e.ended_at > time
        || (e.audio.is_none() && e.transcript.is_none())
    {
        return Err("unsupported_call_export");
    }
    e.call_id = uuid::Uuid::parse_str(&e.call_id)
        .map_err(|_| "unsupported_call_export")?
        .to_string();
    if let Some(a) = &e.audio {
        let path = Path::new(&a.file);
        if path.is_absolute()
            || !path.components().all(|c| matches!(c, Component::Normal(_)))
            || path.extension().is_none_or(|e| e != "wav")
            || a.sha256.len() != 64
            || !a
                .sha256
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            return Err("unsupported_call_audio_reference");
        }
    }
    if let Some(t) = &e.transcript
        && (t.segments.is_empty()
            || t.segments.len() > 64
            || t.segments.iter().map(|s| s.text.len()).sum::<usize>() > 40960
            || t.segments.iter().any(|s| {
                s.text.trim().is_empty()
                    || s.text.len() > 4096
                    || s.text
                        .chars()
                        .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
                    || s.speaker.as_ref().is_some_and(|s| {
                        !label(s)
                            || s == UNATTRIBUTED_CALL_SPEAKER
                            || s.chars().any(|c| ",;<>|&/\\".contains(c))
                            || [" and ", " 및 ", " 와 ", " 과 "]
                                .iter()
                                .any(|p| s.contains(p))
                    })
            }))
    {
        return Err("unsupported_call_transcript");
    }
    Ok(e)
}
pub(crate) fn source(
    e: &Export,
    raw_hash: &str,
    transcript: &Transcript,
    receipt: Option<(&str, u32)>,
) -> work_sessions::Result<(SourceInput, SourceObjectMetadata)> {
    use sha2::{Digest, Sha256};
    let hash = |b: &[u8]| format!("{:x}", Sha256::digest(b));
    let transcript_hash =
        hash(&serde_json::to_vec(transcript).map_err(|_| "call_transcript_encode_failed")?);
    let mut speaker = None;
    for segment in &transcript.segments {
        match (&speaker, &segment.speaker) {
            (None, Some(s)) => speaker = Some(s.clone()),
            (Some(old), Some(s)) if old == s => (),
            _ => {
                speaker = Some(UNATTRIBUTED_CALL_SPEAKER.into());
                break;
            }
        }
    }
    let speaker = speaker.unwrap_or_else(|| UNATTRIBUTED_CALL_SPEAKER.into());
    let audio_hash = e
        .audio
        .as_ref()
        .map(|a| a.sha256.as_str())
        .unwrap_or("none");
    let mut text = format!(
        "Normalized call export v1\nCall identity: {}\nEnded at UTC seconds: {}\nExact export SHA256: {raw_hash}\nAudio SHA256: {audio_hash} (original remains local)\nTranscript SHA256: {transcript_hash}\nSpeaker attribution: reported labels only, never verified identity\n",
        e.call_id, e.ended_at
    );
    if let Some((id, duration)) = receipt {
        text.push_str(&format!("Local transcription receipts: {id}\nModel: gpt-4o-mini-transcribe\nValidated audio duration ms: {duration}\nSpeaker identification: unavailable\n"));
    } else {
        text.push_str("Transcript origin: explicit normalized export; no STT request\n");
    }
    for s in &transcript.segments {
        text.push_str(&format!(
            "\nReported speaker: {}\n{}\n",
            s.speaker.as_deref().unwrap_or("unknown"),
            s.text
        ));
    }
    let external_id = format!("normalized-call-v1:{}", hash(e.call_id.as_bytes()));
    let version = hash(text.as_bytes());
    Ok((
        SourceInput {
            external_id: external_id.clone(),
            version,
            expected_revision: None,
            title: e.title.clone(),
            text,
            speaker: Some(speaker),
            observed_at: e.ended_at,
            valid_from: None,
            valid_until: None,
        },
        SourceObjectMetadata {
            external_id,
            thread_id: Some(format!("call-v1:{}", hash(e.call_id.as_bytes()))),
            parent_id: None,
        },
    ))
}
