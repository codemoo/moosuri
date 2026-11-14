# Local signed installation acceptance

This unit adds an operator-pinned Ed25519 bundle and an actual managed native
installation. It verifies exact manifest bytes, signature, binary hash, native
target, schema, version/sequence and embedded assets before activation. Runtime
and data roots are separate. Linux service/HTTPS/Tailscale files are reviewable
examples; installation does not enable services or change networks.

Updates require a stopped installation and the same Store schema. The existing
exclusive writer lock remains held through encrypted export and fsynced activation.
Interrupted activation blocks normal start until explicit old/new binary recovery.
This is binary recovery, not database rollback. Uninstall removes only unchanged
managed files and preserves data, backups and unrelated or modified files.

Independent review identified two defects and accepted their repairs: mode-only
private paths could admit macOS ACL grants, and a direct Tailscale template expected
Nginx's client-IP header. Opened/created objects and relevant ancestors now use the
existing ACL-aware boundary. Direct Tailscale mode uses the actual loopback peer
and email rate-limit buckets, ignoring forwarded IP headers. HTTPS mode retains
the supplied Nginx single-header overwrite. No per-tailnet-client IP attribution
is claimed.

Native macOS verifier/activation, ACL/topology and actual lifecycle checks passed.
The integrated Linux x86_64 lifecycle also passed with schema44: signed install,
init, served embedded app, active-writer update refusal, stopped encrypted update,
interrupted activation, old-binary recovery with unchanged SQLite bytes, and
data-preserving uninstall. The isolated container allowed executable temporary
files only for the lifecycle and native invitation-process fixture phases.

The combined gate also exercised invitation delivery, native credential execution,
account deletion, schema migration, encrypted restore, backup and broker HTTP.
These use synthetic accounts, local signed bundles and loopback providers. No real
mail, credentials, production service, database, collector or cron was used.

Official signing-key custody/artifacts, Linux arm64, cross-schema upgrade, automatic
traffic/health acceptance, separate vault/broker packaging, devices and live
providers remain release work. This is a development installation path, not full
P10 completion or a stable release.

The final integrated Linux Clippy (`-D warnings`), default app/companion build, generated API type comparison and plan verifier passed. The first Clippy pass flagged the one-time CLI command enum size after the new options; it is explicitly documented as a single startup value. Only the affected Clippy/build/type/plan phases reran; previously passing behavior checks were not repeated.
