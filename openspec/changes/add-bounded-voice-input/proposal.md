# Change: bounded recorded dictation with an editable draft

## Why
F13 and the bounded-STT part of F14 require a usable microphone flow whose preparation does not block ordinary chat. A chat/provider label is not evidence of transcription support.

## What changes
- Add a local recorder with pause/resume, active timer/input meter, preview/re-record/delete and interrupted-track cleanup.
- Normalize a bounded completed recording to canonical mono16kHz PCM16 WAV and validate its exact envelope on the Rust boundary before dispatch.
- Add an explicitly selected installation-managed OpenAI transcription binding and actual phrase-response probe, separate from chat/SIWC providers.
- Add original-session-owned ephemeral SQLite job receipts, conservative billing dedup, expiry/cancellation/provider fences and independent audio seconds budgets.
- Present completed speech as editable text. Explicit insertion appends once to its original composer slot; normal Send remains the only submission.

## Scope
No audio library, arbitrary audio import, local STT engine, realtime/diarization, automatic sending, private service-worker queue, incoming PWA share or live-device/provider acceptance. Schema33 is reserved by root. This isolated baseline31 worker applies33 directly for testing and MUST be combined after root's schema32 before publication; no dummy32 or standalone release is provided.
