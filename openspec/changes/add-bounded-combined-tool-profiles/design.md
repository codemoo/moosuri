# Design

The protocol's shared definition validator accepts at most32 definitions instead
of16, covering the current21-definition read composition with bounded headroom.
All wire dialects use that validator, including the fixed personal `moosuri`
namespace. No definition is silently removed and no registry/parser name changes.

Retain64KiB aggregate schemas/descriptions,16KiB per-schema validation, depth8,
closed required object schemas, duplicate/invalid-name rejection,12 calls per
round,16KiB arguments/results,256KiB private checkpoints,512KiB requests, and the
runtime's existing six-dispatch cumulative fence. Account pins, frozen profiles,
proof admission, credential handling, mutation grants and human mail approval do
not change. No schema migration, new dependency or adapter capability is needed.

Tests exercise32/33 boundaries across all encoders and retained aggregate-size
and unknown-call rejection. The integration regression composes actual version5
production knowledge/Gmail/web/browser definitions plus read-only artifact and
Calendar definitions. Owned loopback providers require every one of the21 names,
strict schemas and pinned connection IDs, then complete plan/shared-compatible
two-round decimal call/result/final continuations. The result is explicitly
synthetic: these tests do not establish full Store admission or external adapter
execution. Preserve actual old-limit RED before changing the production constant.
