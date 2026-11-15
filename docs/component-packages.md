# Separate signed companion and vault packages

`moosuri installation component` creates, verifies and downloads separate native
packages for `credential-broker` (`moosuri-credential-broker`) and `vault-service`
(`moosuri-vault-service`). A downloaded package is ready for an operator to run on
its intended host without compiling. The verifier/downloader is the existing
`moosuri` executable; obtain and trust that executable first through your reviewed
[source or assistant distribution](signed-download.md).

This is a package delivery path, with external signing and an explicit mirror/pin.
The project has not published official signed artifacts or an official signing
key/mirror. No command here deploys a service, provisions TLS/OS identities,
automatically pairs a companion or opens private assistant/vault/device data.
Package authenticity means the independent signing pin authenticated these exact
bytes and metadata. It does not establish device trust, OS code signing,
authenticated build execution, code safety or a vault security boundary.

## Download, verify and run explicitly

Use a new destination whose existing parent is owner-private. Obtain the64hex
Ed25519 public-key pin independently of the mirror and keep its file owner0600.
Choose the actual native target explicitly; downloading on an assistant host for
a different intended vault/device host is supported, without running that binary.
The mirror directory must end with `VERSION/COMPONENT/TARGET/`, where `VERSION`
is the exact signed SemVer with an optional `v` prefix. For example:

```sh
moosuri installation component download \
  --component credential-broker --target x86_64-unknown-linux-gnu \
  --base-url https://mirror.example.test/releases/v0.1.0-dev.1/credential-broker/x86_64-unknown-linux-gnu/ \
  --trust-key /absolute/private/release-public-pin \
  --destination /absolute/private/companion-bundle
moosuri installation component verify \
  --component credential-broker --target x86_64-unknown-linux-gnu \
  --bundle /absolute/private/companion-bundle \
  --trust-key /absolute/private/release-public-pin
```

Transfer the complete verified package and the independently verified pin to the
intended trusted device if necessary, verify again there, then explicitly run:

```sh
/absolute/private/companion-bundle/moosuri-credential-broker \
  --state-dir /absolute/private/device \
  --origin https://assistant.example.test \
  --vault-origin https://vault.example.test --pair
```

Follow [native pairing and selected credential use](native-credential-github.md).
Pairing is a distinct device proof performed with the authenticated user and
short-lived challenge. Downloading or signing a package never pairs it. Keep state,
vault unlock and credentials on the trusted device; no package contains them.

Obtain the vault independently on the chosen vault host/OS identity:

```sh
moosuri installation component download \
  --component vault-service --target x86_64-unknown-linux-gnu \
  --base-url https://mirror.example.test/releases/v0.1.0-dev.1/vault-service/x86_64-unknown-linux-gnu/ \
  --trust-key /absolute/private/release-public-pin \
  --destination /absolute/private/vault-bundle
moosuri installation component verify \
  --component vault-service --target x86_64-unknown-linux-gnu \
  --bundle /absolute/private/vault-bundle \
  --trust-key /absolute/private/release-public-pin
/absolute/private/vault-bundle/moosuri-vault-service \
  --data-dir /absolute/private/vault-data \
  --origin https://vault.example.test \
  --assistant-origin https://assistant.example.test \
  --adapter-url http://127.0.0.1:3211/internal/v1/vault \
  --adapter-key-file /absolute/private/vault-adapter-key
```

That example uses the existing loopback adapter topology. Follow the exact
[separate service/client configuration](vault-service-client.md): the operator
provides a separate OS UID/filesystem rights, origin, reverse proxy/TLS and separate
owner-readable adapter-key copies. A second process under the assistant UID is not
an independent secret boundary. For another network topology, use only a documented
supported adapter configuration. The package does not create one. The vault binary
must be built with its actual React/WASM assets embedded; it needs no Node/Rust/WASM
build tools on the destination host. The signing claim alone does not demonstrate
assets or service behavior; the packager must test the actual built executable.

## Publish a package from reviewed prebuilt bytes

Build the companion and vault from a frozen reviewed source/lock. Build the vault
React/WASM first with [its documented toolchain](vault-service-client.md), then the
native binaries. This is a packager task; recipients consume the resulting package.
Supported target labels are Linux GNU x86_64/aarch64 and macOS x86_64/aarch64.
Only matching 64-bit little-endian ELF executables/PIEs or thin Mach-O executables
with bounded load commands and an executable entry are accepted. Universal Mach-O,
Windows, musl labels and arbitrary executable formats are unsupported. Architecture
and container bounds are checked without execution; the exact ABI/libc, component
behavior and version remain signed packager claims. Test those on the intended host.

Create a new owner0700 directory, copy only the selected executable into it with
its fixed basename and owner0700 mode, then create its unsigned manifest:

```sh
moosuri installation component manifest \
  --component credential-broker --target x86_64-unknown-linux-gnu \
  --version 0.1.0-dev.1 --sequence 1 \
  --binary /absolute/private/companion-bundle/moosuri-credential-broker \
  --output /absolute/private/companion-bundle/manifest.json
cargo run --locked -p xtask -- component-provenance \
  --bundle /absolute/private/companion-bundle
```

Use `--component vault-service` and `moosuri-vault-service` for the independent
vault package. `component-provenance` generates a fresh source-lock inventory and
source/compiler packager claims, binding the exact component, target, version and
binary digest. The source workspace version must agree with the manifest version.
The [existing provenance limits](binary-provenance.md) remain: this is not builder
authentication, a minimal dependency graph, license clearance or vulnerability review.
`xtask verify-component-provenance --bundle DIRECTORY` checks byte consistency only;
it does not authenticate a signing key. Assistant manifests use their unchanged
`release-provenance` path and cannot be mistaken for component manifests.

Sign the **final exact manifest bytes** externally with your independently controlled
Ed25519 key, producing raw64byte `manifest.ed25519`. Never place the signing private
key in the package, repository, data directory or mirror. One external signing
option is the OpenSSL command described in [local installation](local-installation.md).
Then run component verify with the independent public pin before publishing only
these fixed public files under the chosen version/component/target HTTPS directory:

- `manifest.json`, `manifest.ed25519`, the exact component binary;
- `provenance.json` and `source.cdx.json` when the signed manifest binds evidence.

The local verifier refuses unknown adjacent files, unsigned evidence, changed bytes,
wrong component/target and malformed executable tables. No configuration, identity,
secret, deployment hook or private state belongs in this package. Publishing official
binaries still requires release review, complete legal notices and platform acceptance.

## Bounds and upgrades

Component downloads share the [assistant transport policy](signed-download.md):
HTTPS with normal certificate/hostname checks, optional explicit bounded private CA,
no credentials/query/fragment/encoded paths, no ambient proxy, no redirects, fixed
basenames, HTTP200 identity responses, exact signed lengths/hashes and bounded time.
The signature is authenticated before JSON interpretation or evidence/binary requests.
Evidence references are authenticated before inventory fetches. The complete local
component verifier runs before atomic no-replace publication. Ordinary failure cleanup
removes only this invocation's unchanged staging identities and never recurses.
Abrupt kills can leave staging siblings for manual inspection; they are never reused
or swept automatically. A post-publication directory-sync error preserves the result.

There is no component service installer, managed updater, state migration, rollback,
antirollback sequence database or uninstall command. Download/verify authenticates
whichever explicit signed version you selected, including an older one. To change a
running component, separately stop it, preserve its private state/backups, obtain and
verify a compatible new package, then explicitly select/run it. Consult the component's
state/schema and recovery contracts before upgrade/downgrade. Never pass a component
package to the assistant install/update path or run a companion on an untrusted host
simply because its package signature verified. Actual device/OS and official delivery
acceptance remain separate from the package mechanics.
