# Synthetic outcome runner acceptance

2026-11-14. P0/F54 initial public regression runtime; broader evaluation remains open.

The evaluator runs six fresh synthetic worlds through authenticated production HTTP
admission, the actual Runtime, loopback scripted model protocol and file-backed SQLite.
The independent observer/grader checks memory lifecycle, chat continuity/interruption,
saved-evidence calculation with/without a skill, and durable mission plan/execution/
verification/reopen. Task completion, actual durable mission, expected error handling
and apparatus validation are separate counters. No legacy test suite was copied.

Independent review found two concrete false-pass defects: stripping a minus sign from
the final amount, and trusting mission state/receipts without requiring the candidate's
source quote plus source-read/calculation references. Five new observation controls
failed against the original grader on Linux. The repaired grader preserves ASCII and
Unicode signs and independently checks the exact required candidate evidence. Re-review
accepted the fixes. A Clippy collapsible-if failure was repaired without changing behavior.

The final Linux source passed evaluator tests, all-target evaluator Clippy with warnings
denied, an actual CLI report and plan metadata verification. The report records:

| Observation | Result |
| --- | --- |
| Planned/executed/passed/failed/not-run worlds | 6/6/6/0/0 |
| Applicable task completion | 4/4 |
| Actual durable mission (subset of tasks) | 1/1 |
| Expected interrupted-stream handling (not task completion) | 1/1 |
| Independent grader controls (not extra product worlds) | 16/16 |

The initial native five-world development version also passed; final six-world and
oracle repairs were verified on Linux. Counts overlap and are not a product quality score.
Source/config are pinned by compiled byte digests rather than an unchecked HEAD label:

| Digest | SHA-256 |
| --- | --- |
| Source/build inputs | `4be5cb116c7e05feb9ab3d351e5d75a1963f843fb437bda7fefef69526c12a0e` |
| Fixtures | `9f8def7829373950ede682dd03e5ce4773cc78751d01f30ebf7ec9c9774b1030` |
| Profile | `33b5e3368927732a2351a6106af0ca89230554fed7db255f034860c5274492ea` |
| Grader | `ed20da358787016989ac2589d10c3bd3c23f3128a7a29602f0d6d191f709e8a9` |

Observed admission/terminal/workflow percentiles include sample counts and failed observed
work. They measure a small scripted regression, not real-model latency or a load profile.
Cost, unobserved stages, baseline/non-inferiority, semantic quality, blind holdout, the
32-world pilot and live/device acceptance remain explicitly unavailable/not run.
No provider credential, external effect, deployment, original collector/service change or
CI activation occurred. [Usage and limits](../agent-evaluation.md) describe the exact scope.
