# Explicit connected read missions — synthetic outcomes

The bounded implementation adds explicitly named owner Gmail search/read/thread
and primary Calendar list/read to shared-provider one-off/recurring missions.
Old profiles gain no read grant; missing required domains cannot become a reduced
knowledge-only task. Personal-subscription background remains unavailable.

Focused local outcomes:

- `cargo test --locked -p moosuri-store connected_mission -- --nocapture`: **3/3**.
  Composed owner/foreign-decoy and original-mode checks, durable normal logout,
  original-session stable pause/resume and logout fencing, exact app/credential
  refresh CAS, disconnect and late-result/private-checkpoint clearing; required
  domain failure, recurring logout admission and original-pin reconnect denial;
  pause with started dispatch retaining minimum receipt and rejecting late content.
- `cargo test --locked -p moosuri-agent connected_missions -- --nocapture`: **2/2**.
  Real Runtime/model protocol, actual loopback Gmail search/raw-MIME read and primary
  Calendar list/read, fresh run evidence and human-facing evidence projections,
  foreign credential/history exclusion, a durable recurring child after logout,
  late Calendar disconnect retiring historical and pending candidates and no send/
  draft/Calendar-write actions. Separate actual current-request parsing saves daily
  connected reads, keeps missing/dropped domains unavailable, and rejects send
  requirements even when a scripted proposal tries to strip the required effect.
- Web TypeScript checking passed; strict OpenSpec validation passed.

Early fixture failures were corrected: recurring admission requires the scheduler
sidecar enqueue before the mission poll, and Gmail get-message requires raw MIME
rather than a payload projection. No product fallback or extra budget was added.

No real Google/provider/account/credential/history, consent-policy, broad language
quality, production deployment or independent-review completion is claimed. These
are synthetic authority/protocol/privacy outcomes. Final review/integration remains
with the root change owner. No schema or public DTO change is required.
