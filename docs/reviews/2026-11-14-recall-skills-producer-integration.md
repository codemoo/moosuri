# Conversation recall, scheduled skills and local producer — 2026-11-14

This joins source-pinned independent reviews of three bounded implementation units:
[conversation recall](../chat.md#older-conversation-recall),
[scheduled skills](2026-11-14-scheduled-skill-lifecycle.md), and
[local work-session producer](2026-11-14-local-work-session-feed.md).
The current Store schema is47. No real account, private conversation, live provider,
mail send, device Push or legacy deployment was used or changed.

## Independent acceptance

The scheduled-skill implementation retains the actual SkillRead receipt hook and
original profile pins. Discovery alone does not establish a dependency; a disabled
used pin pauses only affected schedules. Original-version recovery requires explicit
resume, and public snapshot collection retains admitted/historical consumers.

The independent recall reviewer accepted typed discovery/read admission and the
fixed-point recall/learned withdrawal closure. Every returned candidate adopts
upstream provenance, including unread snippets; inherited revocation edges cannot
authorize a new read. Missions and artifact-writing profiles remain excluded.

The producer review found and closed two defects: actual server restart could
permanently block local cursor recovery after a committed batch, and unsafe mutable
ancestors were not all rejected. R1 verifies the original batch receipt and exact
current sequence/checkpoint, preserves the old unknown run, and checks the directory
chain. Run-start ACK loss still retains intent and stops; it is not auto-reconciled.

## Integrated verification

The allowlisted public source ran on an isolated Linux container with no network,
read-only source, no production volumes and an unprivileged user. Source hashes were
verified. Affected checks passed:

- Scheduled-skill lifecycle1, existing recurring-mission17, Store recall4 and actual
  Runtime/profile recall2. Counts are overlapping filters, not a quality score.
- Schema/restore filter initially passed19 cases including actual current-schema
  sealed restore. One old schema34 fixture failed because it retained the new recall
  table. Its predecessor teardown and the schema45 followup fixture were corrected;
  both targeted regressions passed. No production migration change was needed.
- Local-session connector5 and the actual Store/router restart regression1. The
  latter commits one source batch, faults finish, reopens the real Store, observes
  unknown/409, and reconciles the original receipt without another ingest or finish.
- All-target app/agent/server Clippy with the optional credential feature, default
  embedded-web app build, actual producer CLI help, exact generated API type equality
  and plan metadata verification.

The scheduled-skill React15 tests passed on its frozen source. The joined web typecheck
and production build passed. Formatting, strict OpenSpec and public source/new-history
secret scans are required at publication. No unchanged broad suite or failed live
model probe was repeated. The initial failed fixture remains recorded rather than
being presented as a fully successful first pass.

This acceptance does not establish semantic history recall, general history GC,
all-client work-session coverage, live assistant quality, production migration or a
stable release. Existing model/device/recovery limits remain in current status.
