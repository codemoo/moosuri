# Device-local Gitea login and issue count

The experimental companion supports **Gitea 1.24.6**, on one explicitly selected
self-hosted origin, and the open issue count of one exact repository. It uses the
owner's local vault login username/password and TOTP, a separate headed Chromium
profile and the existing signed native read job transport. The server receives an
opaque site selection, repository scope and numeric count; no account fields,
password, seed/code, cookies, DOM, CDP or browser error enters the server/model.
The original direct chat waits and receives the signed count without another model
call. General browser missions and other sites/forms remain unsupported.

## Connect once

Use the existing [pairing and live vault/archive unlock](native-credential-github.md),
and start the companion with `--local-chromium /absolute/trusted/chromium`.
Select a **login** vault item whose URL is exactly the origin, username is the
Gitea login name (not an email alias), secret is its password and TOTP is the
account's base32 seed or `otpauth://totp/` URI. Empty stored expiry metadata is
required. The selected item is checked before use; live mode authenticates the
current header and selected ciphertext again after decrypt and before entry.

```text
select-gitea ITEM_UUID https://gitea.example.test OWNER REPO
browser-start
browser-confirm
```

Inspect the new blank Chromium window before the one-time local confirmation.
Only HTTPS origins are accepted, apart from explicit loopback HTTP for a local
self-hosted instance. No certificate bypass, existing profile or remote debugging
TCP port is available. The local grant pins the exact origin, item/revision/key,
site selection and repository. Account or site ambiguity is an error, never an
implicit choice. An edited/rotated Gitea item needs local retirement and selection;
this adapter does not refresh an old grant across revisions automatically.

In the original conversation ask, for example, “Gitea owner/repo의 issue 수 읽어
줘”. The native credential chat feature and actual model tool probe must be enabled.
Settings → Credentials also lists the exact connected repository and offers the
same typed read. A compatible second request reuses the binding and authenticated
local session. Password/TOTP are never requested in chat or web settings.

Locked/offline/unconfirmed browser requests retain the original wait and deadline.
Browser recovery still opens a blank profile, requires local confirmation and
preserves the original eight-hour/30-minute idle/five-minute handoff limits.
Reads and polling do not extend those limits. A browser failure closes the local
session; an entered operation with an uncertain result is unknown and never
submitted again. Restart follows the existing native journal reconciliation.
Device revoke and lock stop further local admission and close the owned browser.

## Pinned adapter and scope

Before credential input the public login page configuration must expose exactly
`1.24.6` as its asset version, which upstream derives from its application version.
The authenticated rendered repository page must expose the same exact asset
version before its account and count are read. Upstream derives this field from
`AppVer` by replacing only `+` with `~`, so an exact `1.24.6` match pins the version
without parsing a footer or sending a separate API credential. Gitea's API auth
group does not accept web session cookies; strict `REQUIRE_SIGNIN_VIEW` instances
therefore keep `/api/v1/version` inaccessible to this browser-only connection even
after successful web login. This adapter stays on the authenticated web surface. The adapter accepts only `/user/login` with the known username
and password inputs and same-path POST form, then `/user/two_factor` with the known
passcode input and same-path POST. It generates one current code after observing
that challenge. CAPTCHA, passkeys, recovery codes, external login, iframes,
unexpected navigation and malformed forms fail closed; no automated retry or
broader MFA bypass is provided. The native UI can be used for supported local
browser lifecycle actions; unknown challenges require the human to resolve the
site separately and reconnect rather than becoming model instructions.

Chromium Fetch interception rejects requests outside the exact origin before
navigation/input. The returned repository page must have the exact selected path
and logged-in username. Only the known issue-tab decimal label is projected as
`GiteaRepository { open_issues }`. Missing count on an existing issue tab means zero;
abbreviated/nondecimal counts are unsupported, not guessed. No issue title/body,
URL, arbitrary script, generic click, repository write or form submission tool is
exposed to the model. The site and local OS/native process remain trusted; this
is not a defense against a malicious instance serving credential-stealing code
at the very origin explicitly trusted for those credentials.

## Why this service and the API alternative

Gitea is a real MIT-licensed self-hosted application with public versioned source,
password/TOTP forms and private repository reads. This adapter is for an instance
the owner controls or is authorized to automate. It grants no authority over a
third-party hosted service's accounts/policies. The official REST API with a
minimal repository-read token is usually preferable for count-only workflows;
this narrow browser path validates the local password/TOTP boundary where that
path is explicitly selected. No real existing account is used in acceptance.

Authoritative pinned sources:

- [Gitea v1.24.6 source/license](https://github.com/go-gitea/gitea/tree/v1.24.6)
- [Public login page asset version](https://github.com/go-gitea/gitea/blob/v1.24.6/templates/base/head_script.tmpl) and [application-version derivation](https://github.com/go-gitea/gitea/blob/v1.24.6/modules/setting/server.go)
- [API authentication group and strict API middleware](https://github.com/go-gitea/gitea/blob/v1.24.6/routers/api/v1/api.go)
- [Password login form](https://github.com/go-gitea/gitea/blob/v1.24.6/templates/user/auth/signin_inner.tmpl)
- [TOTP challenge form](https://github.com/go-gitea/gitea/blob/v1.24.6/templates/user/auth/twofa.tmpl)
- [Enrollment form](https://github.com/go-gitea/gitea/blob/v1.24.6/templates/user/settings/security/twofa_enroll.tmpl)
- [Repository issue count](https://github.com/go-gitea/gitea/blob/v1.24.6/templates/repo/header.tmpl)
- [Official API usage and authentication](https://docs.gitea.com/development/api-usage)

## Acceptance

The opt-in Store integration test
`actual_gitea_vault_totp_original_chat_and_reuse` takes a private synthetic fixture
file through `MOOSURI_GITEA_TEST_FIXTURE` and a trusted headed executable through
`MOOSURI_NATIVE_BROWSER_CHROMIUM`. It creates an encrypted vault login item,
registers its device-local grant through signed commands, waits in the original
chat, performs actual login/TOTP/private repository count and settles the signed
receipt into that same chat. A second chat reuses the same binding/browser.
Fixture keys are `username,password,totp,origin,owner,repo`; values must never be
printed. The isolated real Gitea test repository has exactly three open issues.
Actual real-account/device and signed release distribution acceptance remain open.
