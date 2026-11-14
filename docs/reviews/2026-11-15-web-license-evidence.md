# Web dependency notice evidence · 2026-11-15

This P10/F55 increment adds a Rust offline maintainer command, not release license
clearance. The scope is exactly `web/package-lock.json`,
`vault-web/package-lock.json` and explicitly supplied real installed dependency
roots. No package install, dependency/lock/schema change, network collection,
provider call, production action, CI or generated evidence cache is part of the
implementation. The checks reused the approved existing Rust1.89 toolchain/target.

## Local verification

Four synthetic tests passed through the actual collector. They verify scoped and
nested identities, two versions, explicit aliases and separate app positions;
exact copyright text/hash preservation and deterministic output; absence of host
paths, raw configuration, scripts and resolved URLs; incomplete optional packages,
missing declarations/notices and conflicting metadata; unsafe package/reference
paths, forged installed identity, unsupported sources and binary notices; and
rejection of symlink roots, scope parents, package directories and notice files.
Changing a local notice changes its local content hash while its recorded source
SRI remains unchanged, making the authenticity limit explicit.

`cargo test --locked -p xtask web_notices`, scoped
`cargo clippy --locked -p xtask --all-targets -- -D warnings` and formatting passed.
No unrelated product test suite was run for this command. Final source review
found TypeScript's conventional `ThirdPartyNoticeText.txt` naming outside the
initial generic prefix selector. The bounded selector and existing fixture now
include that attribution; the affected case was rechecked and the actual report
was regenerated from that corrected final source. Independent integration review
remains the integrating owner's acceptance gate.

The corrected final installed-tree invocation used `--require-complete`. It emitted the
complete report before returning exit1, correctly indicating missing evidence:

| Observation | Result |
| --- | ---: |
| npm lock positions | 281 (web141, vault-web140) |
| Installed positions with matching package metadata | 219 |
| Selected notice files | 217 (including2 third-party notice files) |
| Raw notice text bytes | 755713 |
| Total selected package metadata/notice input bytes | 1230031 |
| Positions missing any required evidence | 66 |
| Absent package positions | 62 |
| Installed positions without candidate notice files | 4 |
| Conflicting lock/installed license declarations | 0 |

The62 absent positions are optional platform packages,31 per app. They remain in
the report rather than being treated as complete because this machine does not
install them. The four installed positions without selected notice files are
`saxes@6.0.0` and `@rolldown/binding-darwin-arm64@1.2.19` in each app. This does not
assert that upstream has no license or that obligations are absent: the exact
source/legal evidence needs review and completion.

The exact source lock hashes were:

- `web/package-lock.json`: `a5ec1904abb45a3a4fd2a6593a00c93ce9434b7034afc406905d3a30f6c39555`
- `vault-web/package-lock.json`: `e3ce771a8c10cba625ed15d250348593b21642b0a9f0e5e8f55350b0ecb768da`

The generated1021621-byte report SHA256 was
`d82ab9e775864c3a4d376acada085815c6ff8a380081a9e6b0efcbc2cc3042af`.
The actual report and local paths remain outside Git; only counts, public package
identities and hashes are recorded here. No generated local evidence is embedded
in an artifact or published by this change.

## Limits and acceptance

This inventory includes all locked app/build/dev/other-platform positions, not a
verified minimal graph of embedded React bundles. Installed identity is local
package metadata, not independent registry authentication. Lock tarball integrity
is recorded rather than claimed to verify extracted bytes. Candidate selection
cannot prove every bundled attribution/source obligation; a declaration and notice
are not legal clearance. Collection assumes stable trusted public sources and
rejects observed unsafe filesystem entries without claiming a concurrent hostile-
filesystem sandbox. Unsupported license metadata/source forms require reviewed
handling. Rust notices, OpenSpec tooling, assets/fonts, signed packaging and the
remaining66 evidence gaps retain their own release gates.

See the [command contract](../web-license-evidence.md),
[Rust evidence](2026-11-15-rust-license-evidence.md) and
[release scope](../public-release.md).

Root independently reviewed the frozen collector and adopted all8 exact file
hashes. The full patch hash, actual local report hash and both integrated lock
hashes matched. No material blocker was found within the documented trusted-tree
evidence contract. Candidate-only reads, scope/path/size bounds, deterministic
reporting, missing evidence and report-before-failure were checked without adding
network execution or broadening legal/authenticity claims.

Final integrated offline Linux passed all4 collector cases, including the corrected
third-party selector, and xtask all-target Clippy with warnings denied. Capsule
source hashes were rechecked. Runtime/React suites were not repeated for this
maintainer-only CLI change.
