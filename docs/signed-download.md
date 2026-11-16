# Download an explicitly pinned signed preview bundle

The Rust CLI can fetch a bundle from an operator-chosen **versioned HTTPS directory**.
You provide the existing independent Ed25519 public-key pin and a new local destination.
For the published project preview on GitHub Releases, use the [guided bootstrap](install.md),
which downloads a pinned archive and handles HTTPS artifact redirects. This Rust
directory downloader deliberately still rejects redirects and needs a directly serving
mirror; it does not complete the stable P10 gate.
The downloaded executable is never run by download or verify. Installation, initialization,
service configuration and startup remain explicit [local installer](local-installation.md) steps.

```sh
moosuri installation download \
  --base-url https://mirror.example.test/releases/v0.1.0-dev.1/linux-x86_64/ \
  --trust-key /absolute/private/release-public-pin \
  --destination /absolute/private/downloaded-bundle
moosuri installation verify \
  --bundle /absolute/private/downloaded-bundle \
  --trust-key /absolute/private/release-public-pin
```

Use the exact version/target directory supplied by your chosen publisher. A literal path
segment must equal the signed manifest's SemVer or `v` followed by that SemVer. There is
no latest-version lookup, implicit official key, key discovery or automatic install/update.
The base URL must end in `/`; query, fragment, userinfo, whitespace, backslash, traversal
and percent-encoded paths are refused. Only literal alphanumeric and `._-~+` path
characters are supported. Do not put credentials or secrets in a URL. Output uses fixed
error codes and never prints the URL, transport error detail or certificate contents.
The URL is public input and remains visible in your command history/process arguments.

The pin is an owner-private regular file containing64hex characters for32Ed25519
public-key bytes, obtained through an independent channel. The destination must not exist,
even as an empty directory or symlink, and its existing parent must be owner-private.
Use an absolute path outside source/cloud/shared storage. The existing installer's owned
path, ancestor, symlink and ACL checks also apply. Download never overwrites the destination.

For an explicitly operated private HTTPS mirror, `--tls-ca-file /absolute/private/mirror-ca.pem`
adds one owner-private, bounded (64KiB) PEM certificate to normal TLS trust. Hostname and
certificate validation remain enabled. There is no HTTP or insecure-TLS mode. The signing
pin remains independent of the TLS root. Ambient proxy settings are deliberately ignored;
choose a directly reachable mirror. All redirects are rejected, including same-origin and
GitHub-style artifact redirects; supply a mirror that directly serves each fixed file with
HTTP200. No credentials or cookies are attached by the downloader.

Only `manifest.json`, `manifest.ed25519`, `moosuri`, and (when signed references bind them)
`provenance.json` and `source.cdx.json` are requested. There is no archive extraction,
remote filename interpretation, shell script, hook, service command or recursive fetch.
The signature is checked over the exact manifest bytes **before parsing the manifest or
requesting the executable/evidence**. The native target, embedded-web flag, product, schema,
version, fixed basenames and signed size caps must satisfy the existing installer contract.
The signed provenance bytes are checked before its inventory reference determines the
bounded SBOM request. Evidence remains a [packager claim](binary-provenance.md).

Requests require HTTP200 and identity encoding. Each body is bounded while reading; signed
binary/evidence lengths must match exactly and SHA256/evidence contracts are verified.
Caps are16KiB for the manifest, exactly64bytes for its signature,256MiB for the binary,
16KiB for provenance and8MiB for inventory. Downloads have a15second connect limit,
120seconds per request and300seconds overall. Bodies are buffered within these caps;
this is a bounded preview downloader, without resume, retries or progress reporting.

A random owner-private sibling staging directory holds only authenticated fixed files.
The complete local bundle verifier runs again before an atomic no-replace publication to
the chosen destination. Ordinary failures/timeouts clean only the staging directory and
unchanged owned file identities created by this invocation; unrelated/replaced entries
are retained. A process kill or power loss may leave `.moosuri-download-UUID` siblings.
These are never reused or automatically swept. Inspect such a directory before manually
removing its exact files. Retry with a new destination if a previous invocation published
successfully but reported a later directory-sync error. Never delete an unrelated path
based solely on a matching prefix. Neither download nor verification opens private app data.
