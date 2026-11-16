# Monitor observation lifetime — 2026-11-16

## Defects and changes

A source update invalidates the child's saved result and sets its mission to
quarantined, independently of the accepted monitor's permission. The old monitor
notice gate checked only that permission and acknowledgement, allowing a previously
saved achieved notice to remain visible/pushable after its result was purged. It
could also create a useless quarantined notice before first publication.

The shared publication gate now requires a matching unresolved observation whose
child is not quarantined/deleted/withdrawn or execution-invalidated. Existing
recurring publication, inbox reads, push dispatch and accepted-push display use
that same gate. Current partial/failure outcomes remain eligible; no achieved-only
filter or new approval layer was added.

An expired reusable match previously retired the old monitor and immediately
returned from proposal handling. The new completed task lost its own proposal.
Reuse now retires an unusable match and continues ordinary matching/proposal
creation. The expiry/withdrawal decision is shared with normal monitor admission.
An expired permission is never auto-renewed. Valid reusable consent, source scope,
owner/account/epoch checks, cadence/expiry and no-change gating remain unchanged.

## Verification

The isolated checkout starts at `c0029f2`. Against that baseline product file,
the revised seven-case monitor suite produced five passes and two expected
failures: an invalidated child was published rather than fenced, and expired reuse
used the inconsistent withdrawn retirement branch (which also returned without a
fresh proposal). The baseline log was retained separately from fixed execution.

With the fixes, `cargo test --locked --offline -p moosuri-store
task_monitors::tests -- --nocapture` passed all7. New cases use actual Store
ingestion, child read/verification, source invalidation and deletion, plus inbox,
push dispatch and accepted-push display authorization. They cover invalidation
before publication, invalidation/deletion after a saved push claim, continued
permission for a single later changed-source review, and repeated-poll deduplication.
A two-source review with one actual read settles partial after three bounded
verification rounds and remains linked/visible. Expired reuse is exercised through
two actual completed tasks, a preserved pending replacement and a new ordinary
acceptance. Existing source/connection/owner/epoch/expiry/stop, no-change, scope and
same-observation no-retry checks remain passing.

Targeted `cargo clippy --locked --offline --no-deps -p moosuri-store --all-targets
-- -D warnings`, direct rustfmt, `git diff --check` and pinned OpenSpec2.0.0 strict
validation passed. Existing ts-rs serde-attribute notices remain non-failing. The
only post-suite cleanup removed an unused helper parameter; Clippy compiled all
targets afterward. Independent frozen review was clean. Root native integration reran all7monitor
cases and scoped lint successfully. Final public-source Linux acceptance is linked
from [the integrated report](2026-11-16-adaptive-continuity.md).

The fixtures touch only synthetic private directories. No provider/model calls,
production/legacy data, cron, service configuration, CI or public push was used.
See [change package](../../openspec/changes/monitor-observation-lifecycle/).
