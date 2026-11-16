# Task-linked saved-source follow-up

A successful one-off mission can propose follow-up only from actual saved-source read receipts. The owner starts a bounded read/check/notify policy with cadence and expiry. This never authorizes mail sending, Calendar writes, browsing, provider polling, memory writes, or wider source discovery. Existing final-preview mail approval remains required.

When every verified source resolves to one current, owner-connected provenance thread, the proposal includes later synchronized messages from that exact thread. Connection configuration and credential epochs are pinned. Otherwise the proposal lists exact saved sources and explicitly cannot detect new source IDs. The optional reuse permission matches exact scope, fixed check template, account and authentication epoch.

The existing reminder scheduler fingerprints local indexed source metadata before child/model admission. Unchanged or unrelated evidence creates no child. A source-version-only revision with identical title, text, speaker, observation and validity fields updates the revision baseline without inference. Thread snapshots inspect at most32 members plus an overflow sentinel; only changed/new members can enter review, at most4 source reads of4096 bytes each. Earlier verified review text is bounded context. Larger changes and removals-only expose explicit needs-review status without claiming full coverage or starting a model call.

Admission stores the observation and fingerprint atomically with the recurring child. Failure or unknown outcome does not automatically retry the same observation. Matching reusable tasks share one active monitor; without reusable permission a later proposal needs explicit acceptance before linking to the existing schedule. Coalescing preserves existing cadence and expiry. An unusable reusable match is retired before considering the current completed task's fresh proposal; expired permission is never silently renewed.

The monitor rechecks owner epoch, parent acceptance, source connection/configuration, expiry and withdrawal before execution, publication, final push dispatch/display, and notification reads. Already-published notices do not retain delivery authority after scope/permission changes or after their child result is quarantined, deleted or invalidated. Current partial reviews remain visible with their actual receipts. Ordinary source invalidation of the original answer does not revoke the independently accepted monitor permission. Parent deletion or cancellation, source/collection withdrawal, and connection changes fence further work. Exact-source deletion stops that scope; removed thread members are an observable membership change.

Observation acknowledgement dismisses its notice. An optional note stays local to the next changed review and grants no additional authority or global memory.

API: authenticated GET `/api/v1/task-monitors`, POST `/{id}/accept`, `/{id}/stop`, `/{id}/observations/{ordinal}/resolve`. The follow-up panel is available in the task workspace and alongside the originating conversation. It shows proposals, the source scope, cadence/expiry, optional reuse, observations and stop/revoke controls. One list panel avoids one request per historical card. A resolved observation can retain an owner correction for the next changed review.

Schema73 follows the real live-guidance migration72 in ordinary startup and encrypted restore. Restore pauses schedules and advances owner epochs; old monitoring permission cannot restart automatically. Already-published notice suppression preserves its immutable original source flag.

See [combined native, browser and real-model acceptance](reviews/2026-11-16-live-guidance-monitors.md).

## 원격 메일 답장

이 문서의 저장 자료 감시와 별도로 [Gmail 다음 답장 감시](mail-reply-watches.md)를
지원한다. 원래 채팅의 실제 읽기/발송 영수증에서 한 Gmail 대화를 선택하며, 수집기를
새로 만들거나 기존 수집 범위를 넓히지 않는다. 원격 메일은 메타데이터 확인 후 새 본문만
가져오고 실제 미리보기를 전달한다. 저장 자료 변경의 모델 검토와 구분한다.
