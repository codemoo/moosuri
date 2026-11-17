# Installation reliability · 2026-11-17

## Product references and scope

Official guidance inspected for concrete behavior, not visual/feature parity:

- [Homebrew installation](https://docs.brew.sh/Installation): explain changes and PATH setup. Existing explicit choices and owned-file preservation remain.
- [Ollama on macOS](https://docs.ollama.com/macos): verify actual CLI availability. File presence alone is insufficient; Moosuri uses real readiness and checks immediate menu launch failures.
- [Docker Desktop troubleshooting](https://docs.docker.com/desktop/troubleshoot-and-support/troubleshoot/): offer a usable recovery entry point. Moosuri retains sanitized error guidance locally, without telemetry upload or destructive reset.
- [Docker Desktop settings](https://docs.docker.com/desktop/settings-and-maintenance/settings/): server login startup and opening its dashboard are separate. Moosuri preserves manual start/stop versus autostart semantics and enables web entries only when ready.
- [Ollama FAQ](https://docs.ollama.com/faq.md): restart-to-update depends on an actual download/update system. Do not imitate that label; retain Moosuri's signed stopped-update/backup contract.
- [Tailscale Serve](https://tailscale.com/docs/reference/tailscale-cli/serve): persistent Serve configuration has a separate lifetime. Do not silently remove it with the assistant.

## Delivered behavior

- Complete initial runtime and guided bind port are staged privately and published
  atomically without replacing even an empty existing destination.
- Setup holds the lifecycle control guard and runtime admission while it mutates
  state; active foreground servers resume without a second Store writer.
- Readiness shares a bounded probe (exact 204 and a bounded embedded HTML page),
  reuses its client while waiting and ends after a total 20-second budget.
- Foreground browser dispatch survives server exec through a bounded helper and
  opens only after readiness. The printed URL remains available.
- Menu diagnostics preserve actionable categories, unavailable web entries are
  disabled, quit cannot be dropped behind a full queue, and immediate menu startup
  failure does not invalidate an otherwise usable installation.
- Detached menu/browser helpers use a separate process group, so closing the setup
  terminal does not close the menu. Stop/removal wait for runtime admission to be
  released after the service manager acknowledges shutdown (at most 20 seconds).

## Verification

- Native installation contracts: 25 passed. English/Korean real-binary PTY
  scenarios: 4 passed, including umask 022, injected failures before/after initial
  publication, exact HTTPS port retention, overlapping controls, foreground reruns,
  no piped secrets and delayed browser dispatch.
- Native signed bundle install/init/update/recovery/data-preserving removal passed.
  Scoped all-target Clippy with test fixtures passed. Only unrelated pre-existing
  ts-rs macro diagnostics remain.
- Real isolated macOS launchd acceptance passed: initial start, autostart off while
  running, manual restart preserving that choice, current installation rerun,
  custom-prefix launcher, one surviving menu after PTY closure, duplicate menu
  suppression, menu exit leaving the server alive, uninstall closing the menu and
  retaining identical synthetic data hashes. Created service/shortcuts removed.
- Independent frozen-source review found a Linux first-snapshot race; fixed by
  retaining the exact snapshot passed into the asynchronous tray creation. Follow-up
  reviews of publication, permissions, shutdown admission and helper lifetime found
  no remaining material issue in the delta.
- Acceptance caught staging permissions that depended on a restrictive umask. The
  builder now requests 0700 at creation; tests force ordinary 022. The Linux PTY
  reader also needed to drain output after a fast successful exit before judging
  a missing prompt. These failures were corrected, not excluded.

Linux x64 passed the 24 applicable installation contracts, all 4 real-binary PTY
scenarios (including umask 022) and scoped all-target Clippy. Mac ARM64/Intel and
Linux x64/ARM64 release builds passed; final application sources match the checked
Linux snapshot. Signed package publication is being finalized. No personal
provider, credentials, DB or existing production service was used. The existing
limits remain: no native menu-click/visual acceptance in this turn, no actual Intel
Mac, physical Linux ARM, Windows WSL2 or Linux desktop panel/systemd-session test.
Cross-build and QEMU results do not substitute for those host checks. No automatic
updater, Apple notarization, diagnostic uploads or CI enablement was added.
