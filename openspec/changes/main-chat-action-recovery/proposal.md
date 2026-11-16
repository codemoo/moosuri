## Why
The main chat can send a new question while retrying its preceding failed run, and it leaves obsolete earlier-turn retry controls available after later turns arrive. A lost retry HTTP receipt also disables cancellation of the already-streaming retried attempt. Thread deletion can trap its blocking dialog forever and overwrite inventory refreshed while deletion was waiting.

## What Changes
Fence same-thread send/cancel/retry admissions until the pending operation settles or canonical history proves its target transition. Restrict retry controls to the current latest eligible turn. Bound DELETE receipts and reconcile missing receipts by GET without automatic replay. Remove confirmed deleted threads from the latest inventory and cancel older list snapshots.

## Impact
React main-chat state and controlled UI tests only. No Rust, database, provider/model/effort, external-effect, session authority or mail-approval changes.
