# Tasks

- [x] Add bounded stopped predecessor snapshot/export and real Store migration handoff retaining original writer admission.
- [x] Add target-byte verification, schema/version/trust checks, durable migration/manager activation fence and forward-only recovery.
- [x] Preserve same-schema update/recovery and data-preserving uninstall behavior.
- [x] Document target-bundle recovery entry point, retained artifacts and downgrade limits.
- [x] Run focused same-schema acceptance and real predecessor CLI lifecycle with migration/manager/publication interruptions and post-migration work retention; record exact platform/evidence.
- [x] Independent boundary review and joined Linux validation.


## Multi-version extension

- [x] Define fixed supported floor47..CURRENT in Store backup compatibility API and
  use it for stopped capture and installer forward recovery source admission.
- [x] Retain target-byte pin, source signed-schema match, original writer lock,
  encrypted pre-update export, pending/start latch and existing activation ordering.
- [x] Extend real-binary fixture assertions to actual47→current>=49, retaining the
  four affected interruption/forward-recovery checks without synthetic schema labels.
- [x] Run the narrow floor predicate and actual47→50 multi-version CLI fixture on joined Linux; the four existing interruption/recovery cases passed.
- [x] Independent boundary review, final Clippy/embedded build and root acceptance of this extension.
