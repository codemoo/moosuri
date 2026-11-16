# Guided preview installation acceptance

The new Rust setup wizard reuses signed installation, Store account bootstrap and
the authenticated provider API. The minimal shell bootstrap only detects targets,
fetches a pinned archive, verifies its checksum and invokes the signed Rust installer.
Existing operating installations, accounts, credentials, collectors and cron were
not used or changed. CI remains disabled.

## Accepted source behavior

- Korean and English numbered flows were exercised against the real binary in
  independent synthetic controlling PTYs. Each creates its first administrator,
  preserves that account on rerun, appends the shell PATH block once and installs
  the CLI launcher. The password is absent from terminal echo and stdout/stderr.
- An unattended process cannot consume piped secrets or create installation/data
  directories. CLI prompts explicitly open the controlling terminal.
- Existing signed-installation signature/target/hash, recovery/ACL and network
  template contracts passed on Linux, together with the new local-mode/state and
  removal-preflight contracts. Local mode binds loopback and does not trust
  forwarded client headers.
- The independent frozen review found a partial-uninstall recovery defect. The fix
  preflights files, records removal intent, checkpoints external file removal,
  preserves large logs without reading their whole contents and fences startup
  during teardown. Follow-up review found that issue resolved and no new material
  defect in the delta. Only two Clippy conditional simplifications followed.

The macOS PTY harness initially required access outside the filesystem sandbox;
its close/drain ordering was also corrected before accepting a successful result.
The default dependency-wide Clippy invocation exposed pre-existing optional broker
warnings; the CLI's own all-target `--no-deps -D warnings` check passed. Those failed
attempts are retained locally and are not counted as accepted tests.

## Release/platform acceptance

Platform builds, pinned release publication, bootstrap download verification and
runtime evidence will be recorded here once completed. The source implementation
does not by itself establish those outcomes.

No live paid-model calls, real OAuth login, system daemon deployment, physical ARM
or WSL2 host acceptance is claimed by the synthetic PTY checks. Optional model
verification uses the ordinary authenticated API; actual provider eligibility and
service-manager/session availability remain environment-specific. Stable-release
P10 and separate vault/companion delivery are not completed by this preview.
