# Feature stability — 2026-11-16

Baseline: `6f3183b42a0c944306d3357c767a7a8cb7c4e5e9`. Existing feature stability,
without new capabilities or production changes, is the focus of this unit.

## Changes and reproduced defect

The existing 15 HTTP/Runtime/SQLite synthetic worlds now support six selectable
feature groups. Format 4 records selected and excluded case IDs, feature counts,
applicable oracle controls and the selection-bound profile digest. Default mode
retains all 15 worlds and the exact prior 55 required control names. Missing rows,
failed setup or controls and unselected features cannot produce a passing feature
claim. The evaluator still accepts no provider, credential or database inputs.

Recurring polling previously admitted a second child after the first began waiting
for user context. The regression failed on the baseline: the next due poll returned
1 admitted child when 0 was required. The fix excludes same-schedule unfinished
children before per-owner ranking. A later-due unrelated schedule still runs;
latest-only coalescing, cancellation release, original deadlines and subsequent
normal completion remain intact. No migration or permission change is required.

The [stability matrix](../testing.md#기능별-안정화) distinguishes current contract
evidence from model, provider, device and long-running acceptance. Memory mutation
storage boundaries have bounded review coverage; semantic target selection is
still a live-model gap. The earlier 7/10 result remains historical evidence, not
the accuracy of the current candidate-selection path.

## Acceptance

The first native evaluator run had 13 passes and one setup failure, retained as
`not_run`. Adding a third concurrent real-world test exposed competition for the
Store's process-wide two-slot KDF admission. A test-only mutex now serializes those
three tests; production admission, retries and assertions remain unchanged. The
corrected evaluator library passes all 14 tests, including the actual complete
15-world run and memory-only run. This is not a concurrent-login capacity result.

The recurring suite passes all 19 tests after the reproduced baseline failure.
Independent frozen source reviews of both evaluator and recurring admission found
no material issue. They verify preserved controls, failure accounting, selection,
unfinished states, transaction boundaries, owner isolation and regression meaning.
The recurring query's long-history cost remains unmeasured; it has no dedicated
tenant/schedule history index, so workload measurement remains a follow-up.

The worker's first dependency-inclusive Clippy run stopped at two existing broker
lints (`local_browser.rs` formatting and `policy.rs` argument count). Its scoped
Store Clippy passed; the integrated Store/evaluator scoped all-target Clippy also
passed. No unrelated broker code was changed to satisfy this unit.

Native CLI execution passed the full suite and each individual feature:

| Selection | Passing worlds | Passing observed controls |
| --- | --- | --- |
| default | 15 | 62 |
| chat | 3 | 4 |
| memory | 3 | 7 |
| tasks | 4 | 20 |
| skills | 3 | 18 |
| schedules | 1 | 6 |
| monitors | 1 | 12 |

These are overlapping reruns, not independent quality samples. Selected suites
repeat the common apparatus control. All reports use format 4, record the correct
exclusions, stay under 128 KiB and are created as mode 0600. Six invalid argument
forms are rejected before report creation; reusing an existing report path is
rejected without changing its contents. Evaluator build and source formatting pass.
Evaluated source SHA-256:
`5ff9267de7e4dbb7a26fead2fa79e445108d6c8ae083159b07ae27f52e481303`.

No live model/provider/device run or production deployment is included. Automatic
CI stays disabled.

## Public-source Linux acceptance

Implementation commit `255f318806095140634a9cf769b3b3d3b65f7119` is public and the canonical
checkout was fast-forwarded cleanly. All six grouped Linux phases passed: recurring
Store tests (19), evaluator library tests (13), scoped all-target Clippy, product
CLI/evaluator builds, full evaluator CLI (15 worlds/62 controls), and memory-only
CLI (3 worlds/7 controls with 12 explicit exclusions). The evaluator source digest
matches the native run. The native-only fourteenth library test checks a macOS
legacy capsule and is compile-time excluded on Linux. These are contract checks,
not independent quality samples.

The pinned Rust 1.89 container had read-only source/root, disabled network and no
production paths mounted. Source hashes remained unchanged. Source archive SHA-256:
`78fc8a19ed19bcc39b2480944e3549dd3498489c51fc99904984f6b76306d113`.
Formatting, planning links, strict OpenSpec and source/new-commit secret scans
passed before publication. Automatic CI remains off.
