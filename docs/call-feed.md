# Explicit normalized call feed (F40)

`moosuri calls import` runs once against one owner-selected private directory and
one dedicated custom HTTP source connection. It reads normalized exports, not a
phone/vendor database. It does not discover home directories, record microphones,
place phone calls, run a cron/service or delete remote sources when local files
are absent. Original audio is not uploaded to the source server or archived there.

## Use a transcript export

Create a dedicated **custom HTTP** connection in 자료 연결 and save its ingest token
in an existing owner-only file. Use a separate connection/state for this producer.
The command needs no running local installation and does not open `--data-dir`.

Place the [complete synthetic descriptor](examples/normalized-call-v1.call.json)
in a chosen source directory. This is the repository-owned normalized contract;
no Samsung, Apple, recorder or transcription-vendor format is inferred. Replace
its UUID/title/timestamp/segments with the explicit export you intend to ingest.

```sh
moosuri calls import \
  --directory /absolute/private/chosen-call-export \
  --origin https://your-moosuri-server.example \
  --connection-id 00000000-0000-4000-8000-000000000001 \
  --token-file /absolute/private/call-feed/ingest-token \
  --state-file /absolute/private/call-feed/cursor.json
```

Every path/destination is explicit. Source and state/token parent directories must
be canonical, same-owner0700 directories. State/token/lock/key files must be
same-owner0600 regular files without symlinks or hardlinks. Source files may be0644
inside the private root but cannot be foreign-writable or hardlinked. Opens are
anchored/no-follow at every source path component. State/token/key must be separate,
outside the source root; state must also be outside Git. Ancestor checks and
atomic fsync+rename/lock rules are shared with [work-session ingestion](local-work-sessions.md).

Only `*.call.json` files are considered. Contract version1 has exactly these
fields: `format_version:1`, `kind:"moosuri_call_export"`, stable UUID `call_id`,
positive ended-at Unix seconds `ended_at`, explicit boolean `settled`, nonempty
`title`, `audio` and `transcript`. Unknown fields fail closed. `ended_at` cannot be
in the future. `settled:false` or files modified in the last60seconds defer.

`transcript` is null or `{segments:[{speaker,text},...]}`. Text is literal, never
summarized or treated as instructions. A segment speaker is a reported label or
null for unknown. At most64 segments,4096UTF-8 bytes/segment and40KiB total text
are accepted; unsupported/control-laden/empty text is not partially imported.
A single identical explicit speaker across every segment remains reported
attribution, not verified human/account identity. Unknown or mixed speakers use
an explicit shared reserved marker. Automatic personal-memory extraction rejects
that marker in **both** reported and collection-specified modes, and a collection
cannot claim the marker as its specified speaker. Exporters cannot claim it as a
real segment speaker. No speaker is inferred from filename/contact/account/key.

## Optional bounded audio transcription

To verify an original, set `audio` to
`{"file":"relative/to/chosen-root.wav","sha256":"lowercase-64-hex-digest"}`.
The path is relative to the chosen root, including for nested descriptors; traversal,
absolute paths and links are rejected. The declared digest must match exact file
bytes. A transcript+audio export verifies that audio hash and sends no STT request.

For audio-only exports set `transcript:null`. Transcription requires the explicit
`--transcription-key-file /absolute/private/call-feed/openai-key` option; no key is
read from environment, another app or a model configuration. It opts into sending
audio chunks to the existing fixed OpenAI Audio API, model `gpt-4o-mini-transcribe`.
No caller-selected provider URL, redirect, proxy, automatic retry, codec conversion
or diarization is available. Without a key the file defers with `requires_key`.
Transcribed speakers are always unknown.

Only closed canonical WAV is accepted: mono16kHz signed16-bit little-endian PCM,
exact44-byte RIFF/WAVE/fmt/data header, whole-file size/duration verified from the
sample count, maximum20minutes. No trailing chunks/metadata are accepted. The
producer rebuilds canonical headers for consecutive chunks of at most120seconds;
every original sample remains in exactly one chunk. Each chunk uses the existing
60-second complete request/64KiB response/24KiB text bounds. One invocation admits
at most10chunk requests (at most20validated input minutes); actual processing may
take up to those bounded request deadlines. This is an input/request bound, not a
price or token guarantee.

Before each remote chunk request, its whole-audio hash/index/chunk-hash identity
and opaque local request intent are fsynced. A successful text/hash/duration receipt
is fsynced before proceeding. Definite pre-dispatch `Invalid`/`Unavailable` errors
and explicit HTTP401/403 credentials rejection or429 rate limiting become durable
**retryable** results. The private state retains their first and latest request
UUID/reason plus rejection count in fixed-size history. Repairing the key/network
and explicitly running the same command again may create one fresh intent for
that chunk. There is no within-invocation retry, including when two descriptors
reference the same audio. Existing ten-attempt/invocation and64-chunk lifetime
limits remain; a known rejected existing slot can retry without allocating a new
chunk slot. No extra key, raw provider body or token is saved in its history.

Transport/time-out `Unknown` and malformed/unusable `Response` results keep the
original permanent intent fence: repeating the command reuses confirmed chunks
but never resends that unknown chunk or submits later chunks past its gap. It
does not silently call another provider to resolve uncertainty. Preserve the
cursor; do not delete it to force a retry. Legacy unclassified intents remain
unknown because the earlier failure reason cannot be reconstructed. A separately
obtained explicit transcript may be supplied by the owner for that unresolved case.
A lost ingest ACK is still reconciled before another scan; such recovery does not
itself authorize a fresh STT attempt.

No source is staged until **all** chunks complete. Combined text over40KiB fails
with `call_full_transcript_too_large_no_partial_upload`; no prefix is passed off as
a complete transcript. Known chunk receipts remain private in state. Audio/hash
changes during transcription defer the source; confirmed old-hash results remain
bound to that old audio and cannot become evidence for new bytes.

## Provenance and recovery

The stable source identity derives from `call_id`, scoped by the existing owner's
connection/collection. The source version binds exact descriptor hash, whole-audio
hash when present, full transcript and local transcription receipts. These hashes,
ended-at time and reported speaker labels are retained in source text; existing
connection/scope/run/batch/request/revision provenance is attached by the server.
Local transcription UUIDs are local receipts, not provider receipt IDs. The raw
JSON/audio files remain local originals; this feed is selected text evidence.

Before staging, the adapter checks settled file device/inode/size/nanosecond mtime,
full SHA256 and pinned-root identity again. Changed files defer. Same call identity
updates the existing source revision; identical bytes/receipts are deduplicated.
Conflicting same-call exports within a scan fail rather than silently choosing one.
Use one canonical descriptor per call. A bounded scan is always partial coverage.

The existing custom-ingest engine persists immutable start intent/envelope, looks
up the original receipt after a lost ACK and advances file/object cursor only
with the confirmed batch and original finish/position. It retains revoke/scope/
generation fences and does not rewrite unknown runs as completed. The producer
state discriminator prevents using a Codex cursor as a call cursor. Legacy Codex
states continue with their original default discriminator/recovery semantics.

One scan visits at most4 descriptors,256KiB/descriptor,96MiB total read bytes,
within the shared1024 matching-JSON-file/4096-directory-entry/depth8 limits. Private
state remains at most2MiB,1024 file fingerprints,8192 source versions and64cached
chunk intents/results. Exceeding a bound stops/defer without silently dropping
receipts. Choose a smaller dedicated export scope/connection when needed. Pending
selected text and confirmed chunk text remain private in state for exact recovery;
ingest tokens and API keys are never persisted there. Output contains fixed status,
opaque request UUID and counters only, including attempted/reused/unknown/retryable/requires-key
transcription counts for the staged/recovered scan. No source text/path/key/provider
response appears in logs.

Synthetic checks cover the real Store/router custom-ingest path with a loopback
audio fixture, changed/settled files, dedup/lost ACK, long canonical chunk coverage,
unknown-chunk no resend, explicit401→key repair→later retry, duplicate-descriptor
no-retry and unknown-speaker memory exclusion. Actual recording,
codec conversion, vendor exports, real provider quality/account/rate acceptance,
original-audio archival and whole F40/P6/P7 completion remain separate work.
