# Private PDF text acceptance — 2026-11-14

Accepted scope: bounded text-only PDF attachments through the existing private file
workflow. Pure Rust parser and fixed resource-limited child; no new database schema,
OCR, active PDF execution, form-field/annotation extraction or visual-layout fidelity.

## Behavior and authority

The original PDF bytes/digest and immutable revision share the existing owner-bound
upload lease, quota, retry, deletion, source-context and restore contracts. Text preview
is escaped React content with page markers; download returns the exact original PDF.
Source offsets describe extracted UTF-8 positions. Uploaded content grants no tool or
persistent-write authority, including in follow-up conversations. Concurrent image
preparation and ordinary chat remain independent from file extraction.

The CLI selects its own fixed executable and enters the hidden parser before runtime,
installation or DB initialization. Input/output are bounded pipes with an empty child
environment, fixed working directory and no document-selected path/command. Bounds:
2MiB original,50pages,20,000objects,32MiB decompressed page/font map,24KiB text,64KiB
JSON,5seconds CPU,10seconds wall and512MiB Linux address space. Unsupported forms,
font mappings or malformed/lost content fail without a partial-ready claim. Legitimate
blank/image pages can accompany direct text; image-only documents report no text.
This is resource isolation, not a claim of an OS sandbox against a compromised parser.

## Independent review and repairs

The original review found three defects: borrowed ChildStdin never closed at EOF,
lenient extraction could omit malformed suffixes/fonts/Contents/Form text, and the
extraction call used a256MiB default despite a32MiB load limit. The parent now owns
and explicitly drops stdin; strict bounded preflight validates page/stream/resource
structure and rejects unsupported losses; extraction and font decoding use32MiB.
An independent repair review accepted the25-path production change and executed six
separate synthetic parser fixtures: one supported document plus malformed trailing
content, unknown font, non-stream second page, textual Form and33MiB inflation.
All had the expected outcome.18 new dependency packages passed checksum/file/license
inspection; no pre-existing package was upgraded or removed.

After repair review, three process tests and their test-only module registration were
added. They observe child limits directly and verify cancellation and wall timeout
kill/reap a genuinely stalled child. Production extraction behavior stayed unchanged.
The already accepted vault recovery unit was integrated with only its existing age
entry merged into the PDF lockfile; the public source/closure manifests track this.

## Executed gates

| Surface | Result and observed outcome |
| --- | --- |
| Native and Linux documents |9 passed: page order/Unicode, malformed/partial/unsupported/bounds, actual process limits, cancellation and10second wall-time child termination/reaping |
| Native and Linux private-file Store |10 passed: owner/revision/digest/original bytes, quota, source context, deletion, retry and restore contracts |
| Native and Linux actual CLI/HTTP |3 passed: hidden child skips installation, real parent/child extracts/fails, real authenticated upload/preview/original/retry/cross-account/delete |
| Native and Linux HTTP regression |2 passed: existing file routes remain bounded/authenticated |
| Native and Linux Clippy |affected crates/all targets passed with `-D warnings`; pre-existing ts-rs attribute notices remain |
| React/TypeScript/build |22 targeted tests, typecheck and production build passed; existing chunk-size advisory remains |
| Actual Chrome390×844 |real Rust/SQLite and scripted loopback model: send during pending PDF, preserve draft, preview/download exact source digest, retained chat attachment, unsupported partial result refused, library preview and no horizontal overflow |

Counts are focused filters, not overall product quality scores. The initial Clippy
failure was one collapsible-if style issue. The first Linux run passed six parser tests
but could not execute three synthetic child fixtures on the container's noexec `/tmp`.
The unchanged source then passed with a private UID1000 executable tmpfs only for that
test phase. All phases kept network none, read-only root/source, no-new-privileges and
cap-dropALL; other phases retained noexec `/tmp`. Original failures remain retained.

The initial native Chrome launch was blocked by OS Crashpad sandbox permissions;
a separate temporary profile was launched with the required host permission. Browser
and fixture were closed after verification. This neither used an existing browsing
profile nor contacted a real model/provider. Source, before/after review and loopback
artifacts remain private synthetic evidence; no personal PDF or credentials are public.

## Remaining scope

No real iOS/Android device, real-model answer-quality, OCR/table fidelity, audio/STT,
share-target or production deployment acceptance is claimed. Complex Form/Type3/font
structures are explicitly unsupported in this increment. See [private files](../private-files.md)
for source/data boundaries and [current status](../status.md) for the larger roadmap.
