# Design

One SQLite artifact family has immutable kinds, head CAS, closed typed payloads,
version history, UUID/digest receipts and explicit dependency edges. A separate
schema1 admission sidecar preserves older run tool profiles and prevents capability
upgrades on retry. An isolated current-message classifier precedes retrieval;
Store supplies actual write target, expected revision and command UUID. Exact
literal-title ambiguity clarifies; no free-form model ID selection exists.

All delivered context participates in derived-write provenance. Edits retain
prior edges. Common normal/replay dependency invalidation clears private versions
and dependent runtime payloads. DeleteArtifact uses the already installed typed
destructive journal, not a second journal. Successful local write confirmation
comes from a receipt without another model dispatch.

Markdown preview/export is literal escaped text. UI drafts are in-memory and
account scoped, retained across navigation/conflict and same-request ACK recovery.
No private offline queue, arbitrary files, HTML execution or external publication.
