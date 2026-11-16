## Decisions
The existing promise queue owns bitmap preparation only. After a prepared image has its immutable Blob and preview URL, upload starts through the existing per-item controller/error path without holding the decoder queue. Up to the existing four selected attachments can have independent network receipts; decoder concurrency remains one. Sending ready selections removes only those selection instances, preserving pending uploads and text drafts.

Library refresh owns a separate AbortController, independent of upload recovery controllers and account generation. A newer refresh cancels the previous request and only the current owner can publish data/error or clear its owner. Scope cleanup cancels that read. Deleted IDs remain filtered and successful original-UUID upload recovery keeps its existing refresh path. No GET failure automatically retries a write.

## Limits
These are controlled frontend network/preparation tests. Existing browser and server limits still apply. This change does not introduce upload timeouts, persist raw media or preserve selections through a full page reload. Real device/provider acceptance and integrated browser smoke remain parent-owned.
