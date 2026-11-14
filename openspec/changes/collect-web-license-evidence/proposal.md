# Offline web dependency notice evidence

## Why
P10/F55 has Rust notice evidence but needs corresponding local evidence for the two React apps before maintainers can prepare complete release notices.

## What Changes
Add a Rust `xtask web-notices` command for committed npm lockv3 plus explicitly supplied installed dependency roots. Preserve exact installation identities, declared labels, selected candidate notice texts/content hashes and explicit completeness flags. Add report-then-fail completeness mode, scoped synthetic checks and one actual installed-tree observation.

## Impact
Only a new xtask module/minimal CLI registration and own docs/spec change. No dependency, lock, schema, network install, cache publication, CI, artifact/provenance change or legal-clearance claim. [Contract](../../../docs/web-license-evidence.md).
