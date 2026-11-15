# Native approved Gmail sending · 2026-11-15

The experimental trusted-device sender reuses the existing local draft, full
content/timing preview and explicit final mail approval. A separate selected
Google grant supplies the device-local credential. The server has no token
fallback, and a model can prepare a draft but cannot approve or send it.
See the [supported workflow and limits](../native-approved-gmail-send.md).

## Integrated outcomes

Linux tests exercised the real Store, signed device commands, native journal and
adapter against synthetic loopback provider responses. No actual Gmail account,
credential or recipient was used. The six native outcomes cover:

- Exact approved bytes and scheduled timing, unknown delivery and bounded Sent
  equality observations without another send POST.
- Lost commitment ACK, two devices, precommit revoke, lost settlement ACK,
  companion restart and late authentic receipts.
- Actual scope/mailbox mismatch rejection and a queue with thirty-two stale
  requests followed by a fresh approved action.
- Current encrypted checkpoint/restore: uncommitted work is cancelled, connections
  are quarantined, and committed recovery retains its original identity.

Five native cases passed on the first compiled integration. The restore fixture
initially failed before snapshot creation because its recovery directory was not
private. It now uses owner-only directories and passed without weakening product
checks or assertions. The actual schema55 checkpoint upgraded to56 and retained
quarantine; the existing current-schema restore also passed.

Existing Google device outcomes passed9, mail outcomes19 plus the repaired
schema29/30 predecessor case, and Sent Runtime outcomes4. The predecessor fixture
now removes actual later-schema objects before declaring an old version; no
migration was relaxed. The native draft-only model capability case passed.
All-target Clippy for the affected crates and Linux assistant/companion builds
passed. Exported Rust API types exactly match the React declarations. The React
build and all10 mail UI outcomes passed; the writer's four-file18-test check is
additional local evidence, not a separate product-quality score.

## Independent review

Review of the frozen implementation led to retiring stale uncommitted requests
before applying the queue limit, filtering current connection/binding authority,
and scoping abandonment to the exact action revision. Restore now applies55 then56
and retains committed recovery identity. Independent review accepted those
repairs and the integrated checked SQLite revision conversions. Existing read
broker session authority remains unchanged. Formatting, plan metadata, strict
OpenSpec and changed-source/new-history secret checks are publication gates.

## Remaining acceptance

This is synthetic execution evidence. Real Google OAuth eligibility, scopes and
quota, delivery to a recipient, physical devices, official signed distribution
and deployment remain separate gates. The transport is default-off. Outgoing mail
content intentionally remains in the private assistant draft/action store; the
OAuth credential stays on the selected device. This is not a zero-knowledge mail
composer or a general browser/credential executor.
