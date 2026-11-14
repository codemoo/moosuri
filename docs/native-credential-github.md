# Trusted-device GitHub metadata adapter (experimental)

This slice supplies one executable credential-backed operation: exact
`https://api.github.com/repos/{owner}/{repository}` metadata GET using a personal
API token held only in a trusted native process. It is behind the existing
`credential-broker-foundation` server feature; default server routes remain absent.
A dedicated personal page at `/settings/credentials` starts durable requests and
shows waiting, completion or unknown. The typed agent facade is shared by that
explicit user operation. General model-tool/chat/Gmail/Calendar/browser credential
routing and the Google adapter remain open. This is progress on P8, not P8 completion.

## Trusted-device connection

Build the workspace's `moosuri-credential-broker` binary from reviewed source.
No signed binary/installer or independent signed distribution is provided (P10).
Use a private owner0700 local directory and an existing HTTPS assistant origin:

```sh
moosuri-credential-broker --state-dir /absolute/private/device --origin https://assistant.example.test --pair
```

The companion prints only its public Ed25519 key. Paste it into the personal
connection page, then paste the resulting60-second public JSON challenge into the
companion. It verifies the pinned HTTPS origin, owner context and public key before
key-possession proof. The existing owner-private exclusive journal and separate key
are persisted before pairing delivery; lost pairing responses never silently create
a second identity. Reopen without `--pair`; absent/corrupt state fails closed rather
than resetting it. Transport rejects proxies, cookies and redirects and signs each
challenge-bound exact command. HTTPS/TLS installation remains operator-managed.

For a live vault, also pass its independently hosted HTTPS origin at startup:

```sh
moosuri-credential-broker --state-dir /absolute/private/device --origin https://assistant.example.test --vault-origin https://vault.example.test
```

Then use the native connection commands:

```text
connect-live account@example.test
items
select ITEM_UUID github-owner repository-name
```

`connect-live` asks for the assistant account password on the local hidden TTY and
sends it only to the pinned assistant login. It obtains a one-time handoff for the
exact configured vault origin, exchanges it directly for an in-memory vault session
and discards the assistant cookie. The separately requested vault master password
stays local to the actual vault-core engine; it is never an HTTP field. Pairing owner,
authenticated vault owner/header/inventory and encrypted item are verified before
selection. The assistant receives only opaque binding versions and repository scope.

For explicit snapshot use, export an encrypted vault archive using the existing
vault client, copy it to an
owner0600 single-link file inside a private0700 trusted-device directory, and use:

```text
unlock /absolute/private/vault-copy.json
items
select ITEM_UUID github-owner repository-name
```

`unlock` asks for the archive password on the local TTY with echo disabled. Never
put that password or token in argv, browser, chat or a configuration file. The
vault-core archive manifest and every encrypted item are authenticated before
selection. The archive owner/tenant must match pairing. Select an API-key item
whose URL is exactly `https://api.github.com`; TOTP and nonempty stored expiry
metadata are unsupported in this slice. Only the selected secret field reaches the
adapter. The exact canonical lowercase GitHub owner/repository grant is selected
locally once; no per-request confirmation is added. Multiple compatible devices
are treated as ambiguous rather than choosing another credential automatically.

While unlocked, the companion polls authenticated pending requests every2seconds.
`poll` also wakes it explicitly. Compatible requests automatically reuse the local
selection. `lock` drops the live session or archive engine/root/plaintext ownership; waiting work
stays on the assistant, without a model/worker slot. `quit`/process exit also drop
keys. There is no OS-keystore/background unlock. The caller's OS/process remains
trusted, and no physical memory erasure claim covers external OS copies.

## Live freshness and reconnection

Every new live dispatch checks the authenticated vault session/current head and
fetches current authenticated ciphertext. The local grant pins vault origin, vault
UUID, selected item UUID/revision/ciphertext digest and current header/key lifetime.
After the writer commitment and local decrypt, one final fresh read checks both the
header and selected item before adapter entry. A selected-item edit/deletion,
password/recovery rewrap, root rotation, owner mismatch or authentication rejection
retires the in-memory session/root. Run `connect-live` and `select` locally again.
The same item/repository/vault/origin can refresh its existing opaque binding ID;
epoch/grant versions increase without extending expiry or scope. Changing the
selected item ID requires retiring the old connection and making a new one.

A timeout, network loss or service outage suspends dispatch and keeps a valid
unlocked in-memory root/session. The next poll automatically validates the current
vault again; it does not ask for another password solely because the network was
down. A same-process commitment that has not entered the adapter may resume only
after that validation. Process restart always follows retained-work reconciliation
and cannot use this in-memory retry permission. Archive mode never substitutes for
an unavailable live vault and cannot consume a live grant.

Only proven uncommitted, uncancelled pending requests are rebound atomically to the
new binding version; their original invocation IDs remain unchanged. Exact signed
request digests fence delayed commits from the old version, and legacy digest-free
commits cannot enter a refreshed grant. Committed work keeps its original identity
and receipt semantics. The final fresh read is the admission point: a peer change
after that read cannot recall a request that has already entered the adapter.

## Snapshot and scope boundary

An encrypted archive is an explicit credential copy. It does not prove that it is
the latest live-vault header, and live item/key/password changes do not push a
revocation into this copy. Local grants pin archive vault UUID, item UUID/revision
and root generation; a replaced archive with mismatched versions is rejected and
requires a fresh connection, not silent scope widening. Use native lock/revoke and
upstream token revocation for retirement. Device/server/account revocation blocks
new online commitments; it does not claim deletion of historical archive bytes or
provider token revocation. Administrator account/password recovery never unlocks
this archive. Existing malicious-server-inside-local-grant limits still apply.

The only destination is api.github.com with fixed API version, Accept/User-Agent
and Bearer header. Repository components are bounded canonical ASCII identifiers;
there is no arbitrary URL, method, body, header, shell, plugin or redirect route.
Provider bodies/headers/error strings are discarded. Receipts project only numeric
repository ID/star/open-issue counts and boolean private/archive flags. Returned
provider names/descriptions/login/errors cannot echo secrets into the assistant.
No credential or master password enters assistant SQLite, model tool output or the
nonsecret device journal. The assistant does not receive vault item fields/UUIDs;
the owner-private native journal retains the selected opaque item UUID and
ciphertext-only freshness pins.

## Durable waiting and interruption

A browser request UUID creates one owner-scoped durable job and original invocation
tuple; an uncertain response retries the same UUID. Jobs wait for exactly one
compatible registered local repository scope, then retain that device/binding and
identity. No arbitrary effect ID can be committed. Native prepare precedes the
writer's current session/account/epoch/binding checks; accepted commitment and local
entry are fsynced before the HTTP request. Projected results are signed by the paired
key before durable delivery; the writer checks exact commitment and projection.

Restart never redrives retained entries. Settled receipts are resent exactly
until acknowledged; entered-without-receipt becomes unknown. A retained preparation
with no writer commitment is abandoned positively without provider entry; a known
commitment not locally entered settles not-dispatched. No broker/device failover or
timeout inference creates a second entry. Original request identity remains visible.
An already entered read may finish after revocation; it is not called cancelled.
Restoration quarantines pairing/bindings under existing policy and requires fresh
pairing for new use. Account deletion includes these scoped jobs and repository
binding children in local cleanup while retaining existing minimal receipt evidence.

Google API/OAuth refresh/scope/account matching, TOTP/challenge adapters,
general credential tools, native packaging/signing and production real-account
acceptance are explicitly next gates. Existing explicit GitHub chat waiting uses
the same signed result path; this change does not generalize its adapter scope. No real token/provider/account was used for implementation verification.
