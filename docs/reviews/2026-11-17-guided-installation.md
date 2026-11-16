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

The [v0.1.0-dev.2 installer preview](https://github.com/codemoo/moosuri/releases/tag/v0.1.0-dev.2)
publishes four checksum-pinned archives with Ed25519-signed manifests, binary/SBOM
provenance, both project licenses and third-party notices. Runtime source is
`c899c3b566108ab416e364912d3a24b52fbfe5b6`; a subsequent Linux portability correction
only changes the PTY test's raw pointer. Bootstrap pins and documentation follow
in the publication commit. These are packager claims, not independent builder attestations.

| Platform | Evidence |
| --- | --- |
| macOS ARM64 | Native release build/probe, bilingual real-binary PTY setup/rerun and scoped CLI Clippy |
| macOS Intel | Cross-built release; execution unavailable on this host without Intel/Rosetta |
| Linux x64 | Release build, installation contracts and bilingual PTYs, scoped Clippy; actual release init/readiness/embedded app/login/graceful shutdown/doctor in an isolated container |
| Linux ARM64 | Release cross-build; the same release runtime checks under explicit QEMU, without claiming physical ARM acceptance |
| Windows WSL2 | Linux target detection and documentation; actual WSL2 host remains unverified |

Both Linux binaries require glibc2.34 or newer; bootstrap rejects older/musl systems
before download. Mac dynamic dependencies are system libraries/frameworks only.
Selected release notices cover the four-target normal-dependency union of356 Rust
packages, React/ReactDOM/Scheduler and Rust1.89 standard-library licenses. The
source-lock inventory also includes other workspace/build dependencies; its earlier
whole-workspace notice gaps are not retroactively marked complete. Source/new-history
and package text secret scans passed; large binary contents are outside the text scanner.

The published GitHub release has seven uploaded assets. All four archive sizes and
GitHub-reported SHA256 digests match the committed bootstrap pins. The release tag
targets publication commit `eb16c57ea06020de472a0fab6c94b5d72adc948a`.

The actual public `main/install.sh` one-line command then passed in an isolated
macOS ARM64 home with a synthetic account. It downloaded the published archive,
verified the pinned checksum and bundle signature, accepted Korean arrow/number
choices, created the CLI launcher and appended one PATH block. A blank password
created a temporary credential only on the controlling TTY, absent from captured
stdout/stderr. The installed foreground launcher served readiness204 and the embedded
app; login required a password change, protected administration returned403 before
that change and200 afterward. Graceful shutdown succeeded. Interactive uninstall
removed the launcher and retained every private data file with unchanged hashes.
The first removal harness omitted the required controlling TTY and safely refused;
the corrected interactive check used the same installation, without rerunning accepted
download/login checks. Real service managers were replaced by unavailable fixtures,
so this verifies foreground fallback, not host launchd/systemd deployment.

No live paid-model calls, real OAuth login, system daemon deployment, physical ARM
or WSL2 host acceptance is claimed by the synthetic PTY checks. Optional model
verification uses the ordinary authenticated API; actual provider eligibility and
service-manager/session availability remain environment-specific. Stable-release
P10 and separate vault/companion delivery are not completed by this preview.
