# Mission fixture maintenance — 2026-11-16

Frozen product source: `dd443d03a93f09a8205b9e338f80e14713f2eace`.
This correction changes tests only; no migration SQL, authorization or runtime
behavior is changed.

## Confirmed causes

All ten previously reported failures were reproduced individually against the
unchanged source. Seven reconstructed an old database by dropping a manually
maintained subset of current objects and lowering `user_version`. Schema72/73
instruction/monitor objects survived, so normal startup failed with `Unavailable`
when replaying migrations. These were not valid historical database fixtures.

A shared test-only helper now creates the exact registered SQL prefix, copies
the synthetic snapshot's historical columns, restores historical triggers,
checks foreign keys and backs that database into the existing fixture. It keeps
the original Store keys and journal, allowing real restart/migration assertions.
No later authority tables or columns are copied. Historical triggers are suspended
only during snapshot copying, so reconstruction does not replay mutations.

The seven retained scenarios cover schema25/28/29 late-failure rollback, schema51
acceptance/owner scope, completed untyped schema57 history without new planner
authority, schema58 legacy mission without report authority, and schema69 input
upgrade/encrypted restore without answer replay. Original receipts, identities,
existing-data, restart, rollback and no-replay assertions remain. The schema69
fixture's obsolete constant71 assertion now reads the reopened database's actual
`user_version` and compares it against the current schema.

The other three failed inside `commit_agent_step`, before tool execution: the
current read-only guidance policy rejects Browser/MissionAction proposals with
`Forbidden` before creating tool slots. Their old fixtures expected a stored
unsupported tool and only then a failed execution. The updated assertions check
the earlier rejection, no slot, zero tool/HTTP dispatch and zero repairs, while
retaining mail/Calendar ledger emptiness. Where another effect is admitted, the
Calendar negative test still reaches preparation and rejects unrequested create.

## Verification

The exact-ten baseline log is
`/private/tmp/moosuri-mission-fixture-baseline-exact10.log`: seven `Unavailable`
and three `Forbidden` failures. The corrected bounded mission run is recorded at
`/private/tmp/moosuri-mission-fixture-candidate-missions.log`: all133 passed with
`cargo test --offline --locked -p moosuri-store missions:: -- --nocapture`.
After strengthening schema69's actual-database version assertion, that one
upgrade/restore outcome was checked again in
`/private/tmp/moosuri-mission-fixture-current-version.log`.
All databases, accounts, connection material and proposals are synthetic. No real
provider/model invocation, operational data or production service was used.

## Independent maintenance review

Separate frozen review found no material loss of rollback, owner, legacy-authority,
no-replay or no-dispatch coverage. The historical fixture retains exact historical
columns and checks foreign keys; the effect-policy branch still ends in independent
rejection and empty-ledger assertions. Integrated and public-source checks are in
[the continuation review](2026-11-16-continuation-integrity.md).
