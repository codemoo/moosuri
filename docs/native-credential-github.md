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

Export an encrypted vault archive using the existing vault client, copy it to an
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
selection. `lock` drops the archive engine/root/plaintext ownership; waiting work
stays on the assistant, without a model/worker slot. `quit`/process exit also drop
keys. There is no OS-keystore/background unlock. The caller's OS/process remains
trusted, and no physical memory erasure claim covers external OS copies.

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
No credential, master password, item field or vault item UUID enters assistant
SQLite, model tool output or the nonsecret device journal.

## Durable waiting and interruption

A browser request UUID creates one owner-scoped durable job and original invocation
tuple; an uncertain response retries the same UUID. Jobs wait for exactly one
compatible registered local repository scope, then retain that device/binding and
identity. No arbitrary effect ID can be committed. Native prepare precedes the
writer's current session/account/epoch/binding checks; accepted commitment and local
entry are fsynced before the HTTP request. Projected results are signed by the paired
key before durable delivery; the writer checks exact commitment and projection.

Restart/offline never redrives retained entries. Settled receipts are resent exactly
until acknowledged; entered-without-receipt becomes unknown. A retained preparation
with no writer commitment is abandoned positively without provider entry; a known
commitment not locally entered settles not-dispatched. No broker/device failover or
timeout inference creates a second entry. Original request identity remains visible.
An already entered read may finish after revocation; it is not called cancelled.
Restoration quarantines pairing/bindings under existing policy and requires fresh
pairing for new use. Account deletion includes these scoped jobs and repository
binding children in local cleanup while retaining existing minimal receipt evidence.

Google API/OAuth refresh/scope/account matching, credential rotation against a live
vault service, TOTP/challenge adapters, trusted chat-tool waiting continuation,
native packaging/signing and production real-account acceptance are explicitly
next gates. No real token/provider/account was used for implementation verification.
