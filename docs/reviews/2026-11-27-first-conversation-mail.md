# First-conversation mail request — candidate verification

A newly created conversation could stop at “메일 확인 계속하기” even when the
member had just requested an inbox read. The existing regression reproduced on
public abc37d1c: thread creation and the synthetic typed inbox response completed,
but no Google status or inbox request followed. The selected-thread ref still
held the previous value until React rendered. Updating it alongside the existing
selection state lets the current send continue under the original thread/request.

These three supporting journeys expand C01/C02; they do not add completed primary
journeys or establish real provider authentication/inference.

| ID | User journey | Linux installed result |
| --- | --- | --- |
| FM01 | First message in a new conversation requests mail with an existing connection. | One thread, one interpretation, one read using exactly the original thread/request; result appears and original draft clears. Passed. |
| FM02 | First request needs a connection; return, type a different draft, reload, then resume. | Reload makes no read; explicit resume reads once and preserves the newer draft. Passed. |
| FM03 | Repeat FM02 with a newer draft identical to the original request. | The old request never clears the newly authored identical draft. Passed. |

Actual app login, thread creation, persistence and Chromium UI ran in the retained
marked Ubuntu24 guest. Capability, interpretation and Gmail status/history/read
were intercepted with synthetic responses. Google connection delivery was blocked;
no real credentials, mail, model calls or provider authentication were involved.
Gateway executable SHA-256:
`445e2308728df991aae67f36f22b0e2fbecd499228d254b97f0ed508cbe675f8`.
The 390px screen was inspected; the retained draft and result were visible without
horizontal overflow. The initial harness run failed before any verdict; its failure log is retained.
The final run passed all three cases. No earlier failed run is counted as success.

Linux focused UI checks passed 79 cases; TypeScript, web and embedded native builds
passed. An independent official Claude Sonnet5.5/medium review found no material
defect in the one-line selection fix. Its scope was static product source only;
it did not review or run the installed harness. Existing conversation-switch and
response identity checks remain in place.

Reproduce with `clean-installed-browser.py --vm-root <marked-guest> --output-dir
<new-evidence> --first-mail-journeys` in `deployment/kiri/linux-vm-smoke`.
The runner records only verdicts, executable hash and explicit synthetic boundaries.
This is a tested candidate overlay, not a final-source reinstall. Production
remains dee84754; real Claude authentication, natural-language Gmail acceptance,
automatic fresh OAuth setup and real two-member consent remain open.
