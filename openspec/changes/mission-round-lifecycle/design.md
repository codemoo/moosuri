# Design

The transport driver admits verified terminal outcomes and commits the ordinary
checkpoint/call slots. `mission/round.rs` consumes the same committed round for
fresh and restart work: exact guidance receipt validation/application, initial or
legacy plan admission, saved/pending tool work, current input waits and final
verification. Saved tool results are reused. Fresh provider call order is retained.

Store's guarded `step_has_run_instructions` selects received instructions by
current execution attempt and exact dispatch step. It avoids inferring assignment
from receipt count or acknowledging malformed output. New unassigned guidance can
supersede an old final/input proposal; clearing the proposal never deletes or
applies the new receipt. Input-wait rejection rechecks that guidance in both paths.

Local validation/budget/storage failures use the existing failure mapping.
Owner, epoch, cancellation and acceptance conflicts remain guarded stop conditions;
this is not a blanket terminal write on every failed Store call. Input-request
validation retains its incomplete/protocol outcome. Model refusal/output-limit
handling and historical unverified-capsule rejection remain outside this handler.

There is no migration. Rollback changes code only. Existing current-user grants,
source withdrawal, exact receipt evidence, uncertain-effect recovery and final
mail approval remain owned by their existing Store/domain contracts.
