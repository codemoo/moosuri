## Why

F29/F30/F32 and AUTO01/AUTO02 require durable schedule/control/inbox foundations.
Owners need a useful local reminder flow before scheduling actual model tasks or
external effects. Existing automatic-memory grants establish the background
principal pattern without manufacturing sessions.

## What Changes

- Account-owned fixed local reminders: future UTC once, anchored UTC interval and
  daily IANA wall-clock rules with deterministic gap/fold and misfire policies.
- Five-second supervised bounded DB-only polling, durable occurrence/receipt and
  notification identity, pause/edit/revoke fencing and ordinary restart recovery.
- Authenticated CRUD/history/inbox/quiet-hour APIs and a React creation/control/
  read/dismiss page that preserves account and request lifetimes.

## Impact

SQLite0017 after personal artifacts0016, additive core/Store/agent/
HTTP/React registrations and generated DTO union. Maintained chrono/chrono-tz are
required for explicit IANA rules and recorded TZDB version, with MSRV below1.89.
Account lifecycle and checkpoint/restore integrate their guards separately.
No model/provider/mail calls, natural-language capture, weekly rules, changes-only
observation, Push, topic learning, GC or completed full F29-F32 recovery claim.
