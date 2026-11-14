# GitHub credential read from chat (experimental)

An explicit current chat request such as “GitHub owner/repository 메타데이터를 읽어
줘” can call the typed `github_repository_metadata` tool. Only one exact repository
named literally in the current user message is accepted; canonical
`https://github.com/owner/repository` is also accepted. This is a terminal read for
repository ID/private/archive/star/open-issue metadata. It is not a generic GitHub
API, write operation, code/repository content reader, Google adapter or credential
form. Tokens/passwords/item references never enter chat or a model tool.

The existing experimental server feature `credential-broker-foundation` explicitly
forwards the Store/agent `credential-broker-chat` capability. Admission pins that
flag in private run metadata. Default builds and older profiles do not gain the
tool on retry. Media, missions, artifact-write plans, mixed tool batches and a
second metadata read in one run are unsupported. The useful bounded outcome is
one authenticated metadata result, not an arbitrary composite workflow.

The model sees only a fixed typed owner/repository schema. Store validates the
current run/session/account/epoch and its already-durable tool slot before creating
one broker job. The original chat run UUID and step are reused; a durable mapping
pins the model's opaque call ID to one broker UUID and nonce. The exact same job,
owner/device and signed receipt are observed after browser disconnect, refresh or
normal server restart. Settings-form jobs remain unchanged and default to step1.

If no compatible unlocked native device is ready, chat displays the repository and
an action to `/settings/credentials`. Unlock or first connection happens only on
the trusted device using the existing [native adapter](native-credential-github.md).
An already valid exact-scope binding is automatically reused; no repeated per-tool
approval is added. The assistant never requests a token/password in chat. Waiting
releases the agent lease and is excluded from worker claim selection, so another
conversation can run while this request waits. No LLM checks whether the vault is
open. A Store-only bounded sweep checks original receipts about every2seconds.

Waiting retains the original run's cumulative deadline (normally120seconds), not
a fresh one-hour execution grant. Native mint/commit/entry inherit that expiry.
If it expires before a commitment, chat reports not completed and does not execute.
If it expires after commitment without a receipt, chat preserves outcome_unknown;
no automatic resend or alternate device failover is created. Existing committed
late receipts remain non-authorizing evidence. To ask again, the user creates a
new request rather than retrying a potentially entered old invocation.

On an authentic original signed success receipt, Store fills the same tool slot
and atomically emits a fixed useful metadata summary and completes the same run/
attempt. This narrow result needs no second LLM continuation and does not reset
model/tool/time counters or fabricate prose from an unverified provider response.
The signature, exact stored job, account, epoch, operation, repository, run, step,
mapped call and nonce are checked again before adoption. Unknown, rejected and
positively not-dispatched receipts remain distinct from completed reads.

Cancel/logout/account epoch or suspension, conversation deletion, recovery/restore
and account deletion prevent new authority and visible resurrection. Explicit
cancel expires the original job and cancels its minted invocation; a committed
late receipt can still be retained privately but cannot recreate output. Old
credential attempts are not retryable. Normal restart preserves uncommitted
waiting work and does not repeat model planning. Restore quarantines waits and
requires existing account/device re-enrollment policy.

The archive snapshot/rotation, trusted-native binary/OS and malicious-server-inside-
local-grant limits are unchanged. There is no new signed distribution or live vault
current-head watcher. Google, general chat credential kinds, composite multi-tool
continuation and live provider acceptance remain separate gates; this is a bounded
P8 increment rather than a claim that the whole credential roadmap is complete.
