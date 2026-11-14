## Scope

An explicit normalized ordered event export is the only supported input. There is
no legacy database driver/schema guessing or automatic source exporter. One selected
legacy owner maps to one fresh local enrollment in a new SQLite root.

## Decisions

- Reuse existing authenticated Store mutation APIs/journals so source invalidation,
  memory correction/forget and document dependencies follow current behavior.
  Event order proves which source revision a historical reference used; stale or
  missing references quarantine rather than bypassing current validation.
- Poison and retire a previously staged object when its correction fails. Earlier
  applied events remain counted, while final mapping records its withdrawn state.
- Admit no old authority. Assert execution/provider/connection/schedule/Push tables
  remain empty, disconnect manual source collections and revoke the internal login.
- Use the existing blocked restore-stage prefix and fresh crate-private admission
  token. Partial stages cannot be opened through ordinary runtime/recovery.
- Publish private report directory first and fully verified data root last, each
  with same-parent atomic no-overwrite rename and parent fsync. Orphan reports are
  evidence of an attempt, never data publication success. Existing outputs reject.
- Keep exact-bundle hash and canonical parsed-event hashes. Rejected payload stays
  only in unchanged private input; fixed reason codes do not log private data.
- Store mapping/provenance chunks as ordinary owner-editable private documents so
  normal encrypted DB exports preserve them. They are not immutable provider or
  latestness receipts. Retain the input and separate reports outside backup scope.

## Limits

16MiB/512events, current Store content/revision/reference bounds, checked120second
import loop deadline (no hard OS IO guarantee), and import-time memory/document
created/updated times. Only supplied source observed time/validity intervals are
preserved. Media/vault/settings/work/usage and production parity/cutover are separate.
