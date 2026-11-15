# Design

A distinct manifest purpose binds credential-broker or vault-service, its fixed executable
basename, supported native target, SemVer, sequence and exact size/hash. A shared pure
contract validates strict metadata and bounded ELF/Mach-O architecture/entry/load tables.
It never executes component bytes. Component/version/full ABI remain explicit signed
packager claims; native shape checks are not executable behavioral attestation.

Add installation component manifest/verify/download and xtask component-provenance /
verify-component-provenance. Existing provenance artifact projection gains component fields
only for the component purpose; assistant projection/manifest/update semantics stay intact.
Reuse the existing bounded HTTPS client, fixed-file transport, private owned staging and
atomic no-replace publication. Caller intent supplies component and target; the final HTTPS
directory segments must equal signed version/component/target. Only fixed signed members
are published. Complete verification refuses unsigned/unrecognized local members.

No delivery step deploys a vault, creates accounts/state, pairs a device or starts an
executable. No official key/mirror, automatic release selection, component state migration,
service manager or persisted antirollback authority is introduced. An independently chosen
signing pin authenticates bytes, not device trust or the vault's independent host boundary.
