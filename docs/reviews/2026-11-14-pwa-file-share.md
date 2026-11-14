# PWA file/photo/audio share acceptance

This unit extends the existing one-use text handoff (F16/P7) to bounded photos,
documents and audio. See [PWA bounds and lifecycle](../pwa.md) and the
[delta](../../openspec/changes/add-incoming-pwa-files/proposal.md).

New outcomes cover actual multipart file-only bytes, worker/module restart and IDB
cloning, replay erasure, unsupported/oversized inputs, local previews/removal, capacity
refusal without consume, session retirement, existing account-bound upload queues,
ordinary text while uploads run, explicit audio conversion and active-recording
preservation. Existing text handoff and late-session/origin fences remain in the suite.

Affected frontend scenarios74 and existing public-shell/Push worker9 passed, with
TypeScript (main and worker), production web/worker build and strict OpenSpec.
Independent review found two P2 defects: a composer capacity change during final
authentication could discard already-consumed files, and initial image preview bypassed
existing pre-decode dimensions checks. The repair retains the original window review
within its TTL for an explicit retry, while the durable consumed tombstone prevents
reload/replay; image preview now performs the existing header/48MP preflight and audio
cleanup pauses detached players. Targeted independent re-review accepted both repairs. The repaired IncomingShare
suite19 passed; main/worker TypeScript, production build and strict delta validation
also passed. Repeated suite counts are not a separate quality score.
Tests use synthetic
media/provider inputs. Actual OS-installed share menu, iOS/Android lifecycle and codec
behavior, real speech recognition and end-to-end live model quality remain unverified.
