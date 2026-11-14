# Sealed memory retry execution order

The existing `retry_resumes_a_sealed_local_intent_without_a_replacement_model_call`
regression failed on the pre-personal-ChatGPT baseline in isolated Linux. The run
already held a valid prepared memory intent, but an awaiting mission-classifier
route rejected its previous remote call before the memory planner could resume it.

The first repair assumed generic checkpoint rejection caused the failure. Its
conditional branch was ineffective because explicit retry clears checkpoints and
tool slots. Linux retained the original failure plus a new negative test failure
(86 pass,2 fail,1 ignored). Independent review traced the actual classifier-route
ordering. The incorrectly specified checkpoint-only negative test was removed;
all original memory tests remain byte-identical to the published baseline.

The revised repair resumes the typed accepted intent unconditionally before model
client construction or unrelated classifiers. It reuses Store lease/intent/target
checks and the delivery lock. No intent leaves the existing runtime paths intact.
No provider capsule becomes authority, no unknown remote work is automatically
replayed, and no duplicate memory command is created.

Independent re-review accepted the revised source and confirmed both original
findings resolved. Matching-source Linux Rust1.89 gates passed: full agent suite
87 passed/1 explicit live-network test ignored, Store memory commands32 passed,
and agent all-target Clippy. The existing prepared-intent retry completes with
zero replacement model calls; authority, cancel/shutdown and read-tool retry
regressions remain covered. Both failed runs and the first review are retained.
No new live-model or pending-target-selection acceptance is claimed.
