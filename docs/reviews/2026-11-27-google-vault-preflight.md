# Installed Google Vault preflight — six supporting journeys

Baseline public source:83534457. Production remains dee84754. The existing
marked Ubuntu24 guest has the verified same-Kiri candidate gateway and browser
image; its Vault and Google broker source are unchanged from dee84754.

These six journeys expand V01–V06 in the existing61 primary journeys. They use
actual installed HTTPS services, the real browser crypto worker and private
synthetic fixture members. Each live sealed Deliver request is aborted at the
browser transport boundary before the gateway can start Google login. The cancelled
packet is then sent once to verify stale replay refusal. This is
fault injection, not a mock provider or a successful Google connection.

| ID | Journey and observable acceptance | Result |
| --- | --- | --- |
| GV01 | No Google item; wrong Vault unlock is refused; correct unlock requests account entry; no Chrome starts. | Passed on installed candidate |
| GV02 | Save a Google identity different from member sign-in; persist encrypted and produce only the sealed handoff. | Passed on installed candidate |
| GV03 | Reopen with one item; no account/password prompt; lost delivery retries the exact ciphertext; cancelled replay is refused. | Passed on installed candidate |
| GV04 | Two eligible accounts and a lookalike URL; explicit second-account choice only; mobile chooser remains usable. | Passed on installed candidate |
| GV05 | Other member cannot see items, request the prepared recipient or inspect the job; closing the popup cleans the prepared key. | Passed on installed candidate |
| GV06 | Owner logs out during selection; popup closes, pending key is removed and the old session is unauthorized. | Passed on installed candidate |

The harness preserves original fixture data and records only verdicts, candidate
hash, sanitized failure location, HTTP status numbers, prior-evidence hash and
synthetic item titles. Newly created synthetic encrypted items are
retained and named in its result; inspect them before rerunning an empty-state
journey. No real credential, provider login, OAuth setup, consent or inference
is part of these checks. Final-source installation and real two-member Google
acceptance remain separate gates.

## Observed result and retained failures

GV01–GV04 passed in the first functional run. GV05 initially failed because the
harness expected401/403/404/410. The actual Vault adapter maps the gateway's410
for a foreign recipient to503 `vault_unavailable`. The corrected check requires
same-owner200 with the exact account/challenge before and after the rejection,
and a foreign503 body containing only that fixed error. A never-issued challenge
returns the same fixed503; the response deliberately does not distinguish absence
from failed recipient lookup. This verifies non-disclosure, not an explanatory
authorization status. Claiming the owner's
account in a header also returns the same rejection: session endpoints derive
authority from the cookie, not that header. The initial401 expectation for this
second check also failed and is retained. No product auth change was needed.

A bounded resume kept the four earlier passes and original encrypted items,
then passed GV05 and GV06 without recreating those items or a Google login.
The final resume added a positive one-container/zero-Chrome control before
closing or logging out, then verified zero remaining containers and processes.
Its finalizer also records `cleanup_clean` and fails if cleanup is unconfirmed;
logout also revoked job access. A390px chooser screenshot was visually inspected:
both choices remain readable/selectable with no horizontal overflow.

The gateway executable SHA256 was
`9f5e0e1a466b330c84fc96195ad68a6c92ab193f13f16e892a7c19166d58b385`.
The separate Vault and Google broker source is unchanged from dee84754.
This is an installed candidate overlay, not a final same-source fresh installation.
A prior harness dispatch-placement error ran no journey; it was corrected before
the functional run. Failure artifacts were preserved, never rewritten as passes.

Reproduce on the marked two-member guest with
`deployment/kiri/linux-vm-smoke/clean-installed-browser.py --vm-root <guest-root> --output-dir <new-evidence> --google-vault-journeys`.
For a run stopped afterGV04, pass `--google-vault-resume <prior-google-vault-result.json>`;
it requires exactly those four recorded passes and the same running gateway hash.
Those earlier results are inherited from the prior evidence, not re-proved. Keep the
previous evidence and retained encrypted items; do not reset a Vault to rerun it.

Independent official Claude Sonnet5.5/medium review completed (4 turns).
Positive inventory, unknown-challenge controls and explicit final cleanup were
added and GV05/GV06 passed again. The harness now checks plaintext after every
step and pins cancelled replay to410. Earlier GV03/GV04 evidence proves the
reported UI/ciphertext behavior, but does not retrospectively prove the expanded
per-step plaintext checks or exact replay code. GV02's generated-password check
was observed; resumed runs do not regenerate or know the retained item secrets.

The unchanged-source statement was checked with `git diff --name-only
dee8475486ee366567db219ec7a2ac307928abc8..8353445711d1b2de2c12ed0ca455b3e0562772d4 --
vault-web apps/moosuri/src/runtime_google.rs apps/moosuri/src/runtime_google
apps/moosuri/src/personal_gateway/google_connection.rs
apps/moosuri/src/personal_gateway/google_connection`; it produced no paths.
No runtime code, production data, provider account or configured model changed.

