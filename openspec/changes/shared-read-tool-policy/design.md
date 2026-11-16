# Design

`ToolCapability` classifies exact built-in names and typed requests; unknown names
fail closed. `ReadToolPolicy` holds the three supported upper bounds. Runner
exposure and Store's typed admission use those shared predicates. Exhaustive typed
matches make a new request variant require an explicit classification.

Capabilities are necessary but never sufficient authority. Mission connected-mail
selection, pinned Calendar profiles, recipe route/argument checks and original
effect/report contracts remain local. Task monitors still require exact current
source/revision membership, byte ranges and aggregate read budgets.

A live channel stays read-only before guidance arrives, after application and when
the receipt cap disables new submissions. Store checks durable channel/receipt
identity, rather than treating instantaneous UI eligibility as a permission flag.
Only the existing supported foreground profiles can enable such a channel.

This repairs exposure/admission drift (including mission parcel-history/native
read advertisements) and adds no provider calls or schema migration. Existing
private state is unchanged. Rollback restores the former filters without data
conversion; the lost admission fence should be considered when rolling back.
