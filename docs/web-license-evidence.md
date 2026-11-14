# Offline web dependency notice evidence

`web-notices` helps maintainers prepare dependency notices for the two React apps.
It reads their committed npm locks and explicitly supplied, already installed
public dependency trees. It performs no npm resolution, install, package script,
cache discovery or network operation. The generated report is preparation evidence,
not legal clearance or a distribution-ready notice bundle.

```sh
cargo run --locked -p xtask -- web-notices \
  --web-node-modules /absolute/reviewed/web/node_modules \
  --vault-node-modules /absolute/reviewed/vault-web/node_modules \
  > /absolute/output/web-notices.json
```

Both dependency roots must be absolute real paths without symlink ancestors,
`.`/`..` components or trailing separators. If an existing project path is a
symlink, supply the reviewed real directory itself. The command does not silently
follow that link or search global npm caches. Run against stable trusted public
sources: observed symlink checks do not provide a hostile concurrent-filesystem
sandbox or independently authenticate package contents.

## Inventory and local evidence

The current supported shape is npm package-lock version3, fixed public
`registry.npmjs.org` package sources and string license declarations. Workspace/link,
other registry/git/file sources and unsupported metadata shapes fail with a fixed
error. Each app must have its expected root name/version metadata. Every lock
installation position is retained, including development tools, nested/scoped
packages, aliases, distinct versions and optional dependencies for other platforms.
Alias identity is the lock's explicit package name; ordinary identity comes from
the installation position. Installed package.json must match that exact name/version.
Unexpected installed identity, malformed positions or unsafe paths fail closed.

For each lock position, the report preserves:

- App and source-relative lock position, exact package name/version, lock integrity
  label, development/optional flags and lock-declared license string.
- Raw package.json SHA256 and its installed license declaration, when installed.
  Arbitrary scripts, descriptions, configuration and other metadata are not copied.
- Selected license/notice paths, exact UTF-8 texts and SHA256 content hashes.
- Explicit per-component missing metadata/declaration/notice, missing referenced
  notice and conflicting declaration flags, plus an evidence-complete indicator.

The report also hashes both exact lock files and gives aggregate component,
missing-package, missing-evidence and collected-byte counts. It contains no tool-
inserted host paths, timestamps, registry URLs or environment/configuration values.
Components and notice paths have deterministic order. Exact third-party notice
text is preserved, including its own attribution, URLs and whitespace.

Candidate files have case-insensitive LICENSE, LICENCE, COPYING, COPYRIGHT or
NOTICE basenames, optionally followed by `.`, `-` or `_` suffixes. Conventional
ThirdPartyNoticeText, ThirdPartyNotices, third-party-notices and third_party_notices
basenames are included with the same suffix rule. Files anywhere
under `licenses`, `licences` or `legal` directories are also candidates. The bounded
scan visits package directories but skips nested `node_modules`, whose packages
have their own lock positions. Only package.json and selected candidates have
contents read. `SEE LICENSE IN ...` references from lock/installed declarations
must be safe package-relative candidate paths; arbitrary config paths such as
`.npmrc` are refused. Missing referenced candidates are reported, never invented.
Non-UTF-8/NUL-containing notices, special files or observed symlinks fail rather
than replacing text with a guessed license.

A source lock's integrity is a tarball SRI label. This command records it and
compares installed name/version; it does **not** compare unpacked files against
the tarball or authenticate a registry download. Changing a local notice changes
its reported SHA256 without making a false integrity-validation claim. An operator
must separately establish trustworthy downloaded sources. The JSON metadata parser
uses npm's ordinary JSON object semantics; hashing preserves the exact input bytes.

## Completeness and bounds

Missing installations remain explicit inventory entries. Optional target packages
absent on this machine are not silently excluded or declared licensed. A present
package with absent/unreadable package.json or mismatched identity is invalid input,
not an inferred replacement. Lock/installed declarations are kept separately and
an exact-string conflict is incomplete evidence; the tool does not normalize SPDX
expressions or choose between licenses. A declaration found in either source plus
candidate text is evidence, not proof that the text satisfies every obligation.

Add `--require-complete` to emit the full report and then exit nonzero if any
component lacks package metadata, any declaration or candidate notice, has a
missing referenced notice, or has conflicting declarations. The default command
emits that same incomplete report successfully. Zero missing evidence means only
that these local candidate checks succeeded for every lock position. It does not
establish complete bundled-code attribution, license compatibility/choice, source
provision duties, third-party asset rights or legal clearance.

The bounds are8MiB per lock,10000 lock entries per app,2MiB per metadata/notice
file,64MiB total selected metadata/notice input,200000 scanned directory entries
across both trees,16 directory levels per package and512 selected notices per
package. Paths are bounded and traversal/symlink components are rejected before
selected-file reads. Installed trees must remain stable while collected.

Keep generated reports with private release-preparation artifacts rather than a
stale committed snapshot. Missing platform packages need separately reviewed
public source evidence; this tool does not download them. It does not automatically
embed notices, alter a signed release/provenance format or run a legal gate in CI.
OpenSpec tooling, project assets/fonts and other non-app dependencies remain
outside this command. [Rust notice evidence](rust-license-evidence.md),
[source-lock SBOM](dependency-inventory.md), [binary provenance](binary-provenance.md)
and [public release gates](public-release.md) retain their distinct purposes.

The [dated verification record](reviews/2026-11-15-web-license-evidence.md) reports
actual local coverage and remaining gaps. No complete P10/F55 acceptance is claimed.

Optional [`--supplement`](notice-supplements.md) attaches reviewed external text
to exact name/version/SRI positions with explicit unverified source provenance.
The bounded demonstration covers4 installed positions from2 identities while all
66 original gaps and strict failure remain. It does not authenticate platform
packages, resolve declaration conflicts or replace missing installed metadata.
