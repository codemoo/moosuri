//! Call adapter for the shared private-file/custom-ingest state machine.
use super::*;
use crate::{
    audio::AudioClient,
    calls::{
        Counts, Rejections, RetryReason, Transcription,
        format::{self, Segment, Transcript},
    },
};
use std::{collections::BTreeSet, path::Path};
const MAX_EXPORT: u64 = 256 * 1024;
const TOTAL: u64 = 96 * 1024 * 1024;

pub(super) async fn scan(
    root: &files::Root,
    state: &mut State,
    time: i64,
    state_path: &Path,
    key: Option<&Path>,
    audio: &AudioClient,
) -> Result<files::Scan> {
    let identity = root.identity()?;
    let mut paths = root
        .discover("json")?
        .into_iter()
        .filter(|(_, p)| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().ends_with(".call.json"))
        })
        .collect::<Vec<_>>();
    if let Some(after) = &state.scan_after {
        let offset = paths.partition_point(|(id, _)| id <= after);
        paths.rotate_left(offset);
    }
    let key = if let Some(path) = key {
        let path = transport::credential_path(path, 4096)?;
        if path.starts_with(root.path()) || path == state_path {
            return Err("transcription_key_must_be_separate_outside_source");
        }
        Some(path)
    } else {
        None
    };
    let mut counts = Counts::default();
    let mut attempted_chunks = BTreeSet::new();
    let mut out = files::Scan {
        entries: vec![],
        metadata: vec![],
        files: BTreeMap::new(),
        objects: BTreeMap::new(),
        scan_after: state.scan_after.clone(),
        examined: 0,
        deferred: 0,
        unsupported: 0,
        call_report: None,
    };
    let mut used = 0u64;
    for (file_id, path) in paths.into_iter().take(4) {
        out.examined += 1;
        out.scan_after = Some(file_id.clone());
        let raw = match root.snapshot(&path, time, MAX_EXPORT, TOTAL.saturating_sub(used)) {
            Ok(s) => s,
            Err(_) => {
                out.deferred += 1;
                continue;
            }
        };
        used += raw.bytes.len() as u64;
        let export = match format::parse(&raw.bytes, time) {
            Ok(e) => e,
            Err(_) => {
                out.unsupported += 1;
                continue;
            }
        };
        if !export.settled {
            out.deferred += 1;
            continue;
        }
        let recorded_audio = if let Some(reference) = &export.audio {
            let snapshot = match root.snapshot(
                Path::new(&reference.file),
                time,
                crate::calls::wav::MAX_CALL_WAV as u64,
                TOTAL.saturating_sub(used),
            ) {
                Ok(s) => s,
                Err(_) => {
                    out.deferred += 1;
                    continue;
                }
            };
            used += snapshot.bytes.len() as u64;
            if snapshot.fingerprint.sha256 != reference.sha256 {
                out.deferred += 1;
                continue;
            }
            if crate::calls::wav::chunks(&snapshot.bytes).is_err() {
                out.unsupported += 1;
                continue;
            }
            Some(snapshot)
        } else {
            None
        };
        // Re-read exact raw hashes even for dedup: a descriptor cannot hide a
        // subsequently replaced audio file behind an unchanged JSON fingerprint.
        if state.files.get(&file_id) == Some(&raw.fingerprint) {
            continue;
        }
        let mut local_receipt = None;
        let transcript = if let Some(t) = &export.transcript {
            Transcript {
                segments: t
                    .segments
                    .iter()
                    .map(|s| Segment {
                        speaker: s.speaker.clone(),
                        text: s.text.clone(),
                    })
                    .collect(),
            }
        } else {
            let snapshot = recorded_audio.as_ref().ok_or("call_audio_required")?;
            let audio_path = Path::new(&export.audio.as_ref().ok_or("call_audio_required")?.file);
            let Some((transcript, receipt, duration)) = transcribe(
                root,
                &path,
                &raw.fingerprint,
                audio_path,
                snapshot,
                state,
                state_path,
                key.as_deref(),
                audio,
                &mut counts,
                &mut attempted_chunks,
            )
            .await?
            else {
                out.deferred += 1;
                continue;
            };
            local_receipt = Some((receipt, duration));
            transcript
        };
        // A slow transcription does not authorize ingesting a moving export.
        let fresh = match root.snapshot(&path, time, MAX_EXPORT, TOTAL.saturating_sub(used)) {
            Ok(s) => s,
            Err(_) => {
                out.deferred += 1;
                continue;
            }
        };
        used += fresh.bytes.len() as u64;
        if fresh.fingerprint != raw.fingerprint {
            out.deferred += 1;
            continue;
        }
        if let (Some(reference), Some(old)) = (&export.audio, &recorded_audio) {
            let fresh = match root.snapshot(
                Path::new(&reference.file),
                time,
                crate::calls::wav::MAX_CALL_WAV as u64,
                TOTAL.saturating_sub(used),
            ) {
                Ok(s) => s,
                Err(_) => {
                    out.deferred += 1;
                    continue;
                }
            };
            used += fresh.bytes.len() as u64;
            if fresh.fingerprint != old.fingerprint {
                out.deferred += 1;
                continue;
            }
        }
        let (entry, metadata) = format::source(
            &export,
            &raw.fingerprint.sha256,
            &transcript,
            local_receipt.as_ref().map(|(id, ms)| (id.as_str(), *ms)),
        )?;
        if let Some(previous) = out.objects.get(&entry.external_id) {
            if previous != &entry.version {
                return Err("conflicting_call_identity_choose_single_export");
            }
        } else if state.objects.get(&entry.external_id) != Some(&entry.version) {
            out.objects
                .insert(entry.external_id.clone(), entry.version.clone());
            out.entries.push(entry);
            out.metadata.push(metadata);
        }
        out.files.insert(file_id, raw.fingerprint);
    }
    if root.identity() != Ok(identity) {
        return Err("source_root_changed_retry");
    }
    out.call_report = Some(counts);
    Ok(out)
}

#[allow(clippy::too_many_arguments)]
async fn transcribe(
    root: &files::Root,
    export_path: &Path,
    export_fingerprint: &Fingerprint,
    audio_path: &Path,
    raw: &files::Snapshot,
    state: &mut State,
    state_path: &Path,
    key: Option<&Path>,
    audio: &AudioClient,
    counts: &mut Counts,
    attempted_chunks: &mut BTreeSet<String>,
) -> Result<Option<(Transcript, String, u32)>> {
    let chunks = crate::calls::wav::chunks(&raw.bytes)?;
    let mut texts = vec![];
    let mut receipts = vec![];
    let mut duration = 0u32;
    let mut text_bytes = 0usize;
    for (index, chunk) in chunks.into_iter().enumerate() {
        if !root.matches(export_path, export_fingerprint)
            || !root.matches(audio_path, &raw.fingerprint)
        {
            return Ok(None);
        }
        let chunk_hash = hash(&chunk.bytes);
        let id = hash(encode(&(&raw.fingerprint.sha256, index, &chunk_hash))?);
        let (request_id, text, duration_ms) = match state.transcriptions.get(&id) {
            Some(Transcription::Intent {
                request_id,
                rejections,
            }) => {
                Uuid::parse_str(request_id)
                    .map_err(|_| "invalid_saved_transcription_keep_state")?;
                if rejections.as_ref().is_some_and(|r| !r.valid()) {
                    return Err("invalid_saved_transcription_keep_state");
                }
                counts.unknown += 1;
                return Ok(None);
            }
            Some(Transcription::Complete {
                request_id,
                text,
                text_sha256,
                duration_ms,
                rejections,
            }) => {
                if Uuid::parse_str(request_id).is_err()
                    || text.len() > 24576
                    || text.trim().is_empty()
                    || hash(text) != *text_sha256
                    || *duration_ms != chunk.duration_ms
                    || rejections.as_ref().is_some_and(|r| !r.valid())
                {
                    return Err("invalid_saved_transcription_keep_state");
                }
                counts.reused += 1;
                (request_id.clone(), text.clone(), *duration_ms)
            }
            current @ (None | Some(Transcription::Retryable { .. })) => {
                let rejections = if let Some(Transcription::Retryable { rejections }) = current {
                    if !rejections.valid() || rejections.count == u32::MAX {
                        return Err("invalid_saved_transcription_keep_state");
                    }
                    if attempted_chunks.contains(&id) {
                        counts.retryable += 1;
                        return Ok(None);
                    }
                    Some(rejections.clone())
                } else {
                    None
                };
                let Some(key) = key else {
                    counts.requires_key += 1;
                    return Ok(None);
                };
                if counts.attempted >= 10
                    || (!state.transcriptions.contains_key(&id) && state.transcriptions.len() >= 64)
                {
                    return Ok(None);
                }
                let secret =
                    transport::read_private(key, 4096)?.ok_or("transcription_key_missing")?;
                let secret = std::str::from_utf8(&secret)
                    .map_err(|_| "invalid_transcription_key")?
                    .trim();
                if secret.is_empty()
                    || secret
                        .bytes()
                        .any(|b| b.is_ascii_whitespace() || b.is_ascii_control())
                {
                    return Err("invalid_transcription_key");
                }
                let request_id = Uuid::new_v4().to_string();
                attempted_chunks.insert(id.clone());
                state.transcriptions.insert(
                    id.clone(),
                    Transcription::Intent {
                        request_id: request_id.clone(),
                        rejections: rejections.clone(),
                    },
                );
                transport::save(state_path, state)?;
                counts.attempted += 1;
                let text = match audio
                    .transcribe(
                        zeroize::Zeroizing::new(secret.to_owned()),
                        "gpt-4o-mini-transcribe",
                        chunk.bytes,
                        &tokio_util::sync::CancellationToken::new(),
                    )
                    .await
                {
                    Ok(text) => text,
                    Err(error) => {
                        if let Some(reason) = RetryReason::from_error(&error) {
                            let history =
                                Rejections::record(rejections, request_id.clone(), reason)?;
                            state.transcriptions.insert(
                                id,
                                Transcription::Retryable {
                                    rejections: history,
                                },
                            );
                            transport::save(state_path, state)?;
                            counts.retryable += 1;
                        } else {
                            // Transport uncertainty/invalid response keeps the
                            // original fsynced Intent. Never retry that attempt.
                            counts.unknown += 1;
                        }
                        return Ok(None);
                    }
                };
                state.transcriptions.insert(
                    id,
                    Transcription::Complete {
                        request_id: request_id.clone(),
                        text_sha256: hash(&text),
                        text: text.clone(),
                        duration_ms: chunk.duration_ms,
                        rejections,
                    },
                );
                transport::save(state_path, state)?;
                (request_id, text, chunk.duration_ms)
            }
        };
        text_bytes += text.len();
        if text_bytes > 40960 {
            return Err("call_full_transcript_too_large_no_partial_upload");
        }
        duration = duration
            .checked_add(duration_ms)
            .ok_or("call_duration_limit")?;
        receipts.push(serde_json::json!({"chunk":index,"request_id":request_id,"chunk_sha256":chunk_hash,"transcript_sha256":hash(&text),"duration_ms":duration_ms}));
        texts.push(Segment {
            speaker: None,
            text,
        });
    }
    Ok(Some((
        Transcript { segments: texts },
        encode(&receipts)?,
        duration,
    )))
}
