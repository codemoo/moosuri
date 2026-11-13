# Account lifecycle verification

The administrator can suspend/resume a member, change another account's role,
issue a24-hour one-time password, or end all sessions. Schema18 follows personal
artifacts16 and local reminders17. These actions retain personal content and grant
no administrator read access to it. Full deletion and invitation email are separate.

Each mutation rechecks current administrator authority and target revision after
password hashing, inside the existing delivery fence. Durable auth-epoch adoption
precedes the SQLite receipt; failure in between locks the affected account on
restart. A response retry returns the same content-free receipt, never a replayed
password. Password changes and operator recovery advance the management revision;
recovery retains suspension.

## Independent review and repair

Independent review found a UI race: a refresh observing the reset's own resulting
revision could discard the one-time password; a later revision could leave a stale
password visible. The repair binds display and copying to the receipt account,
request and resulting revision. Matching receipts remain visible; superseding
metadata and account switches remove them. The reviewer independently reran8 UI
acceptance cases, including both reproduced defects. No further concrete backend
defect was found in that bounded review.

The earlier isolated Store suite exposed three fabricated-old-schema fixtures;
they had retained new columns while lowering user_version. The fixtures now rebuild
the actual predecessor shape and the7 migration cases passed. Earlier failures
remain distinct from the final integrated checks.

## Integrated checks

React36 account/workspace/reminder cases and the production type/build pass.
Integrated macOS checks pass: Store204 (one explicit reference opt-in ignored),
one additional account/reminder regression, HTTP33 (one explicit live opt-in
ignored), CLI7, and workspace all-target warnings-denied clippy. Linux x86_64
passes64 focused account/schedule/migration/recovery/HTTP/CLI checks (one live
opt-in ignored), using hash-verified source in a network-disabled container.
The only later Rust difference in that Linux snapshot is rustfmt on the added
schedule test; formatting it produces the exact final bytes. Documentation
changes have no runtime effect. No real accounts, tokens or sends were used.

The first local clippy command could not resolve the offline registry because
its external subcommand did not retain the temporary source configuration.
Installing the same verified vendor configuration in this run's private Cargo
home resolved it; the actual clippy run passed. This was tooling setup, not a
product test failure.

The reminder integration also checks the account's active status at enqueue, claim
and publication. Its regression suspends after claim, rejects late publication,
lets another account deliver, resumes the member with fresh login, and verifies
that only an explicit owner resume grants the next future occurrence.
