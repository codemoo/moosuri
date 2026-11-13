# Private images — integrated validation

2026-11-13. Schema21 adds bounded PNG/JPEG/WebP preparation, immutable account-owned
canonical JPEG/thumbnail storage, separate upload lifetimes, private previews and
library controls, and verified image input for the four existing model dialects.
This is image support, not general file/PDF/audio processing or complete assistant acceptance.

Selected IDs/revisions are fixed at send admission and retained for exact retries.
Pending files remain independent of text sending. A synthetic pixel capability
probe is bound to provider revision/model/effort. Server-loaded image bytes enter
only the private wire request; ordinary DTOs and stored run metadata carry references.
Image-derived contexts cannot gain tools or persistent mutation authority.

Deletion uses the destructive journal and withdraws dependent outputs, checkpoints
and evidence. Tests cover owner isolation, cancellation before upload admission,
conflicting retries, pending/late completion, decoder/allocation/quota limits,
immutable provider payloads, failed response retry and image selection ACK races.
Restore preserves ready bytes and tombstones, quarantines processing and clears
vision proofs. Normal schema migration and offline restore migration are atomic.

Independent review found a delayed download could complete after deletion was
acknowledged. Per-download abort controllers, deletion tombstones and lifecycle
generation checks now fence both fetch and delayed blob resolution; cleanup revokes
owned URLs/timers. The repair and schema21 integration passed a second independent
review. Nine independently executed checks included a newly held-blob response,
partial migration rollback, actual current sealed restore and genuine schema19/20
forward restore. The reviewed 40-path manifest SHA256 is
`fa78017358204710423ace3f36d5110dbbf7c4ea0c3e64a6c48de15d3b96fc16`.

macOS integrated verification passed Store media/restore filter13, React196 across
22 files, TypeScript, production build, generated DTO equality, workspace/all-target
clippy and formatting. Linux x86_64 passed Store235, agent59, connectors131, HTTP39
and CLI18:482 passes and5 explicitly optional ignored tests. The isolated Linux
source was checked against the frozen source before and after execution. All365
prior lock package versions/checksums were retained; the18 new image dependencies
had no matching OSV advisory on the review date. This is a dated check, not a
guarantee that dependencies have no vulnerabilities.

A real Chrome journey against the compiled Rust server caught another defect:
the embedded CSP rejected authenticated `blob:` image URLs. The exact additional
one-line change permits `blob:` only in `img-src`; script/connect/frame policies
remain unchanged. This delta received separate independent source review and a
fresh Linux CLI18-pass/1-optional-ignore run. Final CLI source SHA256 is
`98dcafbb2e60a51f08ed6a2db78deedf04279cfb47e3013f9e4e97be51260b8a`.

Using a synthetic640px image and synthetic account, Chrome at390×844 verified real
upload,256px thumbnails,640px enlargement, restart/reload persistence, library
deletion, removal from selected previews and preservation of the current text draft.
The page stayed within the viewport; the expanded composer can scroll to its text
input. Real browser chat/model execution was not attempted because no model was
configured in that installation. A separate actual HTTP test uses a synthetic
pixel-reading provider for image dispatch/failure/retry. Neither replaces live
vision quality evaluation or physical iOS/Android testing.

No personal accounts, production data, old services, cron, live email or CI were
used. PDF/STT/recording/share, image-derived durable mutations and scalable external
blob storage remain separate work. [Usage](../media.md) and [current status](../status.md)
state the supported behavior.
