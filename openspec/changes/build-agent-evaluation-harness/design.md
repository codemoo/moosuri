## Context

Rust/React/SQLite replaces the legacy product. Old test code is not the specification.

## Goals / Non-Goals

Measure real outcomes with small useful suites. Do not reproduce every legacy test, use production data, or claim syntax checks establish assistant quality.

## Decisions

Use synthetic state plus an isolated evaluator oracle. Exercise the actual Rust runner with both fake and real model providers. Score required state/receipts before subjective language quality. Freeze configurations and split related worlds together. Keep safety/effect invariants separate from average task scores. The detailed metrics and cost cadence live in docs/testing.md rather than a duplicate list here.

## Risks / Trade-offs

Model/simulator/judge variation and small samples can produce inconclusive results. Preserve all planned and failed attempts. Trial repetition is not independent world diversity. Add cases when they catch a distinct defect; remove or merge redundant tests.

## Migration Plan

Start with P0 fixture/oracle design, P1/P2 contract vertical slices, then domain model/device checks as implementations become available. Record actual evidence in docs/status.md.

The first Rust release compares against a source/config-pinned legacy implementation running only isolated synthetic worlds through a common fixture adapter. Public runtime/install/PR checks do not depend on that private baseline; disclose reproducibility limits in comparison reports. Later comparisons use the prior supported moosuri tag. Baseline selection and adapter checks precede preregistration and candidate results. No runnable baseline means no non-inferiority pass; historical scores never fill that gap.

## Initial executable slice

`tools/moosuri-eval` owns compiled public inputs, a bounded loopback protocol fixture,
read-only SQLite observations, independent gold/grading and metadata-only reports.
Gold is never imported by request-construction modules or exposed through agent tools.
The server router admits authenticated HTTP requests in-process; the real Runtime
owns execution. Observations include output, tool slots, dependencies, every returned
search candidate and persisted durable mission plan/candidate/state. The independent
upstream ledger and saved slots have a bijective step/call/argument linkage.

These are public regressions, not blind holdout. Negative controls/mutants remain
outside the primary numerator. Failure handling, executable tasks and actual durable
missions have separate denominators. Setup failures remain planned/not-run; execution
errors remain failed with available timings, calls and usage. Reports bind actual
source bytes rather than asserting clean HEAD, provide observed-sample percentiles,
and leave unobserved stages/cost and baseline/quality comparisons unknown.

## Private reference content preflight

The separate `baseline-pin` CLI selects149 runtime/migration/dependency files from
the historically hashed audit manifest and verifies exact size/hash, regular paths
and absence of extra runtime files. It does not import legacy modules, load `.env`,
read DBs/tests/traces, mutate the checkout or start a process. Reports contain only
content identity and check/count metadata. The original source remains private.
An untracked legacy runtime cannot be identified by its checkout HEAD alone.

The preflight is not a freeze or runtime result. A later isolated adapter must copy
only allowed source, recheck hashes, discard Python bytecode and pin provider/profile
configuration. PostgreSQL remains a requirement of the legacy implementation. Its
clock and tool differences require an explicitly comparable time-independent first
fixture set; none of these missing execution gates are inferred from a matching hash.
