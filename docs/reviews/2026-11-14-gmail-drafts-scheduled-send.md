# Gmail drafts and one-off send implementation evidence

The schema40 implementation adds actual Gmail users/me draft CRUD, explicit opt-in
compose OAuth and React controls. Provider writes copy the exact owner-selected
local snapshot; they neither approve sending nor rewrite local approved MIME.
A digest rejects detected stale provider copies. Gmail has no conditional-write
revision here, so concurrent external editor changes can still race GET/write;
the UI/docs explicitly state the limitation. Unsupported MIME, named addresses,
reply metadata and multipart/attachments remain read/delete-only. Subject truncation
and duplicate recipient headers cannot become editable content.

Fixed one-off mail scheduling retains an exact UTC instant/IANA zone alongside the
immutable action. Timing mutation increments revision and revokes approval;
content/connection edit removes timing. Final preview/approval is always human and
shows all supported content plus exact time. Due work uses the existing durable
send commitment and no-network-retry pipeline, current account/connection/source
and pinned background grants. It cannot execute model-generated due-time content.
Logout alone leaves explicit approval active; restore clears grants and cancels
uncommitted actions. Original unknown/reconciliation/late-receipt semantics remain.

## Standalone checks before shared-lane handoff

- Initial three new SQLite schedule tests passed: future/no-approval/due-once,
  timing revision, logout/background authority, invalid account/connection/source/
  grant/scope/cancellation and content-edit timing reset.
- Last affected cross-crate mail run: agent8 passed, connectors16 passed, Store19
  passed and one synthetic predecessor-schema fixture failed because it retained
  schema40 objects while pretending to be schema29/30. The fixture was repaired
  to drop `mail_schedules` and OAuth `compose` before lowering its schema. The same
  mechanical repair was applied to prior restore/schema38 construction fixtures.
- Actual synthetic HTTP service tests exercised create/list/read/update/delete,
  stale provider digest, frozen local approval independence, opt-in/no-IO without
  compose and disconnect while a read waited, with zero send requests.
- React11/11 affected tests passed (`Mail.test.tsx`, `GmailDrafts.test.tsx`): new
  provider CRUD controls/import/unsupported MIME plus exact timing, disabled old
  approval during edit and explicit new-revision approval.
- Main and worker TypeScript checks, production Vite build, strict named OpenSpec
  validation and changed-Rust rustfmt passed. No dependency/lockfile/CI changes.

Independent review found the local snapshot check preceded an awaited provider
read on update. The implementation now rechecks the exact local revision/digest
immediately before every provider save, after provider IO. A held GET -> local
reject -> zero PUT regression was added. Its execution and the repaired predecessor
fixtures remain part of the integrated rerun. The final validity check also fences
expiry immediately before dispatch IO. Shared Cargo access was released for other
work; integrated checks must regenerate the DTO contract, rerun affected Rust/tests
and Clippy, and perform independent platform verification. This document does not
claim those deferred checks passed. TypeScript declarations have been synchronized
by hand for this handoff and need the normal generated-export equality check.

No live Gmail account, OAuth exchange, email send, credential, production DB,
collector, cron or legacy service was used or changed. All test mail addresses,
HTTP origins, credentials and SQLite fixtures are synthetic.

## Integrated acceptance

The root regenerated shared DTOs and integrated schema40 alongside account deletion41
and request continuations42. Linux actual Store/agent/connector mail tests, authenticated
mail HTTP, predecessor migration and encrypted restore checks passed. Independent
review additionally reproduced a stale timing callback enabling the wrong mail's
approval; editor lifetime and identity fencing plus both held-response/reselection
regressions passed. React affected flows, generated TypeScript and production build
passed. The final dispatch limit now follows F36's five-minute lateness window;
independent review confirmed expiry is checked at both claim and dispatch, and the
focused assertion/expired regression passed on Linux. No real email was sent.
