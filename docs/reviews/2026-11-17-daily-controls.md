# Installation daily controls · 2026-11-17

This follow-up applies the same [product-reference review](2026-11-17-installation-reliability.md)
to fewer everyday actions and clearer recovery. It does not introduce another
settings app, telemetry uploader or automatic updater.

## Delivered behavior

- `moosuri open --start` starts an owned user service and opens chat after readiness.
  The desktop menu offers the same action when stopped. With no service, an
  explicit terminal hosts the foreground server and explains Ctrl+C/lifetime.
- `moosuri restart` and the native menu stop the owned service, confirm writer
  release, then start again under one lifecycle control lock. Autostart is retained.
- Health responses identify the managed installation, including setup and busy
  errors. Managed serving checks the exact installed data directory before opening
  Store. Status/start/open/menu distinguish a different responding server from a
  temporarily unready one. Runtime identity is operational metadata, not authentication.
- Service-file collision is checked before saving ownership intent. A new setup
  preserves the unrelated file and can finish in foreground mode and uninstall.
- WSL generated shortcuts use `open --start` and a terminal. Refresh/removal accept
  only exact generated old/new templates for this prefix, including an interrupted
  refresh; arbitrary user edits remain preserved.

## Verification

Native installation contracts passed (26), followed by the affected setup checks
and health response checks after independent review. The six real-binary PTY
scenarios passed with synthetic accounts and ordinary umask022: bilingual rerun,
piped-secret refusal, concurrent control, interrupted publication, service-file
collision/removal, and start/open with a foreign installation on the same port.
Managed serving with a different data directory fails before creating it.

Actual isolated macOS launchd passed start-and-open, readiness-gated URL dispatch,
a new server PID after restart, retained disabled autostart, current setup rerun,
launcher refresh, single menu lifetime, menu exit with live server, and removal
with unchanged synthetic data hashes. All created service/shortcut entries were
removed. Native signed install/update/recovery/removal and scoped Clippy passed.

Independent review found a Store busy response losing the runtime header and
being misclassified as another server. The response builder now attaches identity
on errors as well; busy remains unready and can recover. The reviewer accepted
the fix. Initial Linux acceptance also exposed two test assumptions: a forked
child may briefly inherit a CLOEXEC lock until exec, and uninstall may use arrow
menus. The checks now wait boundedly for actual lock release and select by the
visible removal label; no production assertion or failure path was removed.

Final isolated Linux scoped Clippy, three health/configuration checks, 25
installation contracts and all six PTY scenarios passed. All four release builds
passed. Linux x64 and ARM64 QEMU release runtimes passed initialization, embedded
web readiness, synthetic login, graceful stop and doctor.

The [dev.5 preview](https://github.com/codemoo/moosuri/releases/tag/v0.1.0-dev.5)
retains the independent signing pin and advances the signed sequence. Four exact
archives carry verified signatures, source inventory, packager-reported provenance
and notices for 399 selected Rust dependencies plus React and Rust std. Source,
new history and package text scans passed; opaque executable bytes are outside the
text scan claim. Package publication verifies all seven uploaded asset sizes and
SHA-256 digests before advancing the public bootstrap. Public-bootstrap acceptance
is recorded below after publication.

 Physical ARM/Intel/WSL2, Linux panel/systemd-session
and actual native menu clicks remain separate acceptance limits. Existing personal
accounts, providers, services, collectors, cron and databases were not used.
