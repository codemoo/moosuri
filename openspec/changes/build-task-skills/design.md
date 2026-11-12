# Design

Use the existing run admission and typed tool loop. A private profile inside the stored
run model JSON pins the toolset and bundled recipe identities/content digests. Missing
legacy profiles keep knowledge-only execution. The public model choice remains unchanged.

Compact recipe metadata appears only when its pinned profile is available. `skill_read`
loads exact bounded content; it cannot import arbitrary paths/URLs or create authority.
Original source recipes follow Agent Skills metadata and describe saved-source outcomes.
The proposed moosuri sidecar remains a local compatibility convention.

Decimal arithmetic uses bounded strings and checked integer coefficient/scale operations,
never floating point or expression evaluation. The result has no inherent source authority.
The agent must obtain inputs from the user or actually returned evidence.

Both tools use persisted existing slots/results. They share account/run/provider fences,
6-model/12-tool/120-second limits and derived-content purge. No extra global cache, shell,
model-based supervisor or external mutation permission is introduced.

Public case evidence and planned connector completion contracts live in
`docs/research/personal-agent-tasks-2026-11-13.md` and `docs/task-skills.md`.
Tests separate orchestration/authority/numeric checks from actual natural-language task
understanding. Failed live cases remain recorded with model, effort and adapter limits.
