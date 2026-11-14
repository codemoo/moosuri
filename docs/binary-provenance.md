# Offline release artifact provenance

The release packager can bind exact executable bytes to a declared source checkout,
compiler metadata and a freshly generated source-lock inventory. The existing local
Ed25519 manifest signs that binding. This is a packager attestation claim: it does
not authenticate build execution, prove a compiler produced these bytes, establish
SLSA/builder identity, identify a minimal binary dependency graph, clear licenses or
perform a vulnerability audit. No official key or signed release is created here.

Build from a frozen public checkout and its committed locks, including embedded
React assets. Keep packaging output in a new owner-private directory outside source,
data and served storage. Copy the native binary there as `moosuri`. Generate the
existing unsigned native manifest first:

```sh
moosuri installation manifest --binary /absolute/private/bundle/moosuri \
  --sequence 1 --output /absolute/private/bundle/manifest.json
cargo run --locked -p xtask -- release-provenance --bundle /absolute/private/bundle
cargo run --locked -p xtask -- verify-provenance --bundle /absolute/private/bundle
```

`release-provenance` verifies the manifest's binary size/hash before producing
`source.cdx.json` and `provenance.json`, then replaces only the unsigned manifest
with a reference to the new evidence. It refuses an existing signature, provenance
reference or sidecars. It invokes only local Git metadata commands and configured
`RUSTC -vV` (default `rustc`), not Cargo builds, the artifact executable, a registry,
network, a key generator or a signing command. Run it using the build's intended
compiler configuration; the recorded output describes that queried compiler and
current checkout, rather than proving the preceding build used them. The Git
revision and dirty boolean include tracked/untracked status but omit filenames,
diffs, remote URLs and source content. Ignored build outputs are outside Git status.
Dirty checkouts are reported truthfully and remain allowed for previews; a stable
release's clean build is still a separate release gate.

The SBOM is the exact deterministic output of the existing offline generator from
all four source locks, including other targets, dev/frontend and OpenSpec tooling.
Its complete bytes and size are hashed into provenance. Provenance repeats the
exact binary hash/size and public product/version/target/schema/web metadata from
the manifest; its complete bytes and size are hashed into the manifest. Only the
fixed basenames `moosuri`, `provenance.json` and `source.cdx.json` are admitted. No
metadata-supplied path is followed. Provenance is limited to16KiB and inventory to
8MiB, in addition to the existing16KiB manifest and256MiB binary limits.

Sign the **final modified manifest bytes** externally as described in
[local installation](local-installation.md). Preserve all five files together:
`moosuri`, `manifest.json`, `manifest.ed25519`, `provenance.json`, `source.cdx.json`.
Packaging interruption can leave unsigned sidecars or a temporary file; inspect
and use a fresh bundle directory rather than signing or overwriting an incomplete
one. No private signing key belongs in the bundle or source checkout.

The xtask verification command checks byte binding and evidence structure only; it
does not authenticate signatures. For signature authentication, use the matching
native installer with an independently obtained public pin:

```sh
moosuri installation verify --bundle /absolute/private/bundle \
  --trust-key /absolute/private/release-public-pin
```

That command executes no bundle code and opens no installation data. Install and
update use the same pinned signature and exact byte/evidence validation before
creating installation state, backups or migration markers. Their staging uses an
owned snapshot of validated bytes, so replacing a source path after admission does
not substitute content. A copied native executable's existing bounded public probe
still must match the manifest during staging. Evidence remains in each immutable
slot and is rechecked by managed run/status, update and recovery. Data-preserving
uninstall removes only the verified evidence files along with their signed slot;
changed or unknown content remains retained under the existing rules.

Format1's optional `provenance` member preserves old three-file bundles. A new
installer accepts those without claiming provenance. Reserved evidence sidecars
without a signed reference reject. Old strict installers reject the new field;
use the new bundle's executable to install/update/recover. Selecting an evidence
bundle refreshes the verified manager under the pending fence even at the same
Store schema, so the old manager is not left unable to parse the active release.
Recovery choosing new follows that same refresh path. No Store schema, dependency,
service, data migration policy or automatic CI setting changes.

Synthetic [verification evidence](reviews/2026-11-15-binary-provenance.md) is bounded
to local artifacts. Official signing/distribution, actual native clean release
build acceptance and independent build authenticity remain separate P10 gates.
