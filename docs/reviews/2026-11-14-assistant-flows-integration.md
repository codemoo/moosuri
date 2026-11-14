# Credential chat, parcel lookup and followup notification integration

The schema46 integration combines the accepted native credential chat45, public
Japan Post latest-summary tool and saved followup date notifications. It retains
migration44→45→46 in normal open and sealed restore. Public CI remains disabled.

Independent source review found and repaired two material defects: a second live
companion session could begin dispatch after the original chat session ended; and
a new explicit tracking number could lose to an older number when the model omitted
its argument. Original-session validation now happens in the broker writer
transaction before mint/reissue/first commitment. Current explicit shipment identity
wins at Store admission. Existing commitment recovery remains available.

## Linux acceptance

An isolated Linux x86_64 Rust1.89 container received only allowlisted public source
and built web assets, verified by SHA256. Networking was disabled; production data,
credentials, services and collectors were not used. The affected actual paths passed:

- Broker Store7, credential Store2 and Runtime2, including original-session loss,
  parked-run capacity, restart and same signed result in the original chat.
- Parcel connector1 and Runtime1: official-format synthetic HTML through HTTP,
  typed lookup, current-number precedence and same-thread fresh observation.
- Schedule33 and Push15, including saved date revisions, snooze/coalescing, source
  mute, quiet hours, late work suppression and original inbox links.
- Schema20, sealed restore12 and broker HTTP1, preserving schema46 data and
  quarantine. Filter counts overlap and are not a unique suite or quality score.
- Final all-target Clippy with experimental chat enabled, default application build,
  generated Rust→React type equality and plan metadata verification.

Initial Clippy found a nonminimal test-host expression and the larger parcel
provenance value expanding every external-tool permit. The condition was simplified;
optional parcel provenance is now boxed only when present. The affected parcel path
and final Clippy/build/types/plan passed after these changes; other successful gates
were not repeated. Existing ts-rs serde-attribute notices remain.

The affected React ChatPanel/Artifacts/Schedules30, full TypeScript checks and
production web build passed on this integrated UI. Native author evidence and
independent review are retained in the linked unit records below.

## Supported limits

Credential chat remains default-off and supports an exact GitHub metadata request
through the native archive-backed companion, preserving the original120-second
run deadline without model polling. It is not general Google/live-vault rotation.
Japan Post supports the verified latest-summary shape; only public synthetic
not-found responses were observed live, not a successful real shipment or full
history. Followup notices use explicitly saved dates and the existing five-second
scheduler/inbox/quiet/opt-in Push path; real mobile delivery is not established.

[Credential chat](2026-11-14-credential-chat.md) ·
[Parcel tracking](../parcel-tracking.md) ·
[Followup notifications](2026-11-14-followup-notifications.md).
