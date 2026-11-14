## Why

Local preview manifests authenticate an executable but cannot retain exact
source-lock inventory or declare its source/compiler context. Offline packaging
needs a signed byte binding without claiming authenticated build execution.

## What changes

Generate and verify fixed provenance/SBOM sidecars from an unsigned native bundle.
Extend format1 with an optional signed provenance reference, preserving legacy
bundles. Validate before install/update mutation, snapshot verified bytes, retain
and reverify evidence in immutable slots, and refresh incompatible old managers
under activation admission. No schema/dependency/CI or signing-key change.
