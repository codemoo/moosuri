## Evidence chain

The existing independent Ed25519 pin authenticates final manifest bytes. A strict
optional fixed-name reference binds provenance size/SHA256; provenance repeats exact
artifact identity and binds exact fresh CycloneDX source-lock inventory bytes.
Source Git revision/dirty status and configured rustc verbose version are explicitly
packager claims, not authenticated execution or minimal binary composition.

## Compatibility and state

Legacy format1 omits the reference and remains accepted. Unbound reserved sidecars
reject; old strict readers reject new fields. New install/update captures validated
binary/evidence before state mutation. Staging uses those captured bytes. Existing
slot verification covers evidence for update/recovery/managed run/status/uninstall.
Evidence-bearing same-schema activation refreshes the verified manager under the
pending fence, as schema upgrades already do. Choose-new recovery repeats that
refresh before selection; choosing old never rewinds data. Fixed names eliminate
metadata path interpretation. No account/data/network or service logic changes.

## Packaging failure

Only unsigned manifests may be enhanced. Sidecars and temporary manifest are
create-new; completed file writes precede atomic manifest replacement and directory
sync. Interrupted partial bundles remain unbound/refused. The operator retains and
inspects them and selects a fresh bundle directory. No signing key is read/created.
