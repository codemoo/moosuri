# Why

Earlier recovery journals omit direct thread/mail deletions and command forget
can identify its target only through a newer command slot. A future historical
replayer needs bounded direct opaque intents and an explicit coverage boundary.

# What Changes

- Freeze the exact legacy knowledge prefix and bind one successor journal to the
  installation and that predecessor, preserving authentication recovery.
- Adopt each destructive operation through one fsynced record and atomically apply
  scoped effects, applicable receipts and successor watermark before readiness.
- Export explicit format2 with five mandatory entries and typed coverage while
  preserving format1/four-entry codec compatibility.

# Impact

Store migration0015 and affected effect/export primitives; no new dependencies,
crypto, network/provider calls, restore/import, off-host checkpoint or activation.
Automatic-memory0014 is retained at main integration. The isolated schema13 worker
uses temporary schema14, to become15 with its threshold and constant together.
