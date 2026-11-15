# Trusted local browser groundwork

The native companion can open a separate headed Chromium window on the owner's
device. This is an experimental local session foundation, not a supported website
login/read adapter. It never imports the person's existing browser profile and has
no password/TOTP autofill, model browser tools or server browser transport.

Run the already enrolled companion with an explicit trusted absolute Chromium
executable, in addition to its existing `--state-dir` and `--origin` arguments:

```text
--local-chromium /absolute/path/to/chromium
```

Use `browser-start`, inspect the new blank window, and run `browser-confirm` locally
within five minutes. `browser-observe` returns only the original session identity,
generation, fixed lifecycle times and a bounded page count. `browser-status` reads
only the retained local status. `site_adapter_available` stays false. Neither command
proves authentication or completion of an assistant task. Unsupported page counts
or malformed/stalled browser responses close the session instead of returning raw
errors, URLs or page text.

The browser uses a fresh private incognito profile and an inherited CDP pipe, with
no TCP debugging port. The companion inherits only the local environment needed to
open a desktop window. It does not connect to an existing browser or export cookies,
screenshots, form values, titles, URLs, DOM, arbitrary JavaScript or a debugging
endpoint. Page metadata temporarily received on the device is reduced to a page
count; it never enters the native journal, server, model or community package.
The trusted native process and local OS account are inside the trust boundary.

The existing private, exclusively locked native journal stores one session's
identity, times, phase and owned process group. The original lifetime is at most
eight hours with thirty minutes of idle time and a five-minute initial handoff.
Polling does not renew idle time. Local confirmation cannot renew the eight-hour
ceiling. A monotonic watchdog closes the owned browser even while another companion
operation is waiting. Closing/locking the companion stops the owned browser; normal
`browser-close` additionally confirms process exit before removing its temporary
profile. An unconfirmed exit is reported as unknown, not successful cleanup.

After a companion restart, `browser-resume` is an explicit local command and works
only while the original session and idle deadlines remain valid and the old process
group is no longer alive. It uses a new blank profile and another human confirmation;
old cookies, URLs, form submissions and tabs are not replayed. A crash between the
launch journal and persistence of the process identity remains unknown and cannot
automatically relaunch. Damaged journal state is not automatically recreated.
The companion never signals a process group solely from retained storage.

This slice requires no new user credential approval because it does not use any
credential. The existing [server selected-page workflow](selected-browser-missions.md)
remains a different trust boundary. Connecting an actual local website result to
the original mission requires a separate reviewed adapter with a pinned site/read
task, official API alternative, policy, dedicated test account and outcome evidence.
Only that adapter can define a safe observation projection and any future native
vault password/TOTP use. Arbitrary forms, payments, purchases, calls and government
submissions remain unsupported.
