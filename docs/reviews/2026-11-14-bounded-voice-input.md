# Bounded voice input — integrated acceptance

2026-11-14. Recorder and separately configured OpenAI API transcription are integrated
with full personal ChatGPT schema32 before audio33. This accepts the bounded development
implementation, not live speech quality, physical mobile devices or production deployment.

## User behavior

Record, pause/resume, stop, preview, re-record and delete work independently of text chat.
Only explicit conversion sends canonical audio to the separately verified API binding.
The editable result appends once to its original, latest draft; normal Send remains explicit.
Insertion releases the temporary preview, collapses the panel and focuses the current draft.
Unknown outcomes remain honest and ordinary recovery retains the original UUID/WAV.

## Independent review and repairs

The original review reproduced two defects: disabling and re-verifying the same audio
binding could revive an old transcript or stale probe; lost acknowledgements/status polls
could allocate a new billable attempt. Permanent generation/CAS authority now atomically
retires old work, and explicit recovery reads the original receipt or submits identical
UUID/bytes only when no receipt exists. Original RED and repaired GREEN evidence was
retained; independent R3 review accepted both repairs.

A separate integrated review checked full P1/audio migration order, restore quarantine,
provider/source DTOs, routes/CSP and original-slot ownership. It found a synthetic Calendar
fixture still expecting schema30 after successful full migration; its final oracle now uses
the current version while failed-migration/retained-data checks remain unchanged. A duplicate
DROP in the synthetic legacy mail fixture was also reproduced and corrected. Full P1's
larger provider binding triggered the combined Clippy enum-size gate; boxing that binding
preserves dispatch semantics. Independent repair review accepted these changes.

## Observed verification

- Native full Store run:394 passed, one stale Calendar success oracle failed, two explicit
  reference-Go-age interoperability cases were ignored. The corrected exact Calendar case
  then passed. Those ignored reference tests are not counted as coverage of this unit.
- Final native audio Store7 and actual HTTP2 passed; exact generated DTOs matched24,166
  bytes. Combined six-crate all-target Clippy passed with explicit audio fixtures; the
  production CLI was separately built without test-fixture features.
- Linux audio Store7, sealed restore11, exact legacy Calendar/mail upgrade cases, canonical
  WAV connector1 and actual HTTP2 passed. The final boxed source repeated its affected
  audio Store/HTTP gates and combined Clippy successfully. Earlier failed logs remain.
- Final React recorder, chat, media and provider settings67 passed; frontend and worker
  TypeScript plus production build passed. Counts overlap prior runs and are not a sum of
  unique product or quality scenarios.
- Actual Chrome390×844 used the production Rust/SQLite server, a synthetic WebAudio input
  stream, the native MediaRecorder/decoder and the emitted WAV Worker under actual CSP.
  Preview played; canonical mono16kHz/PCM16 bytes were observed. Text was explicitly sent
  both during paused recording and during conversion. New-thread slot migration, edited
  insertion into the latest draft once, panel collapse/focus, cancellation track cleanup,
  no private API cache, no horizontal overflow and logout draft removal passed.

The browser's audio settings/status response was scripted. Separately, the HTTP2 gate used
real Rust router/SQLite/multipart transport with a loopback provider and generated nonspeech
PCM, including held transcription while ordinary chat completed, billing dedup, deletion,
late probe and settings replacement. Neither fixture establishes actual recognition quality.
No live microphone, provider credential or operating data was used.

## Bounds and remaining acceptance

Local input8MiB, active recording below120s with codec-flush margin, paused lifetime5min,
decoded PCM32MiB/preparation10s, exact mono16000Hz PCM16 WAV3,840,044bytes, upload30s,
provider60s/64KiB response, transcript24KiB and original-session job120s. Processing is
one/account and two/installation. Audio duration uses its own conservative UTC budget:
minimum1s per admitted attempt,600s/account/day and3600s/installation/day. Minimal permanent
UUID receipts are capped at4096/account; general receipt GC remains separate.

Raw audio remains in current-screen/task memory. Temporary transcript deletion does not
promise physical SQLite page erasure or removal from prior encrypted backups; restore
quarantines it before serving. Actual configured API speech/Korean accuracy, iOS/Android
microphone and call interruption remain explicit pending gates. Incoming PWA share, arbitrary
audio files, streaming STT, speaker separation and local engines are separate scope.
