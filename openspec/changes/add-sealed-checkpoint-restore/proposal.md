# Why

A permanently sealed source needs a usable matching destination before its cutover
command can be safely exposed. Restore must preserve full current withdrawals and
quarantine imported execution and authentication authority before normal startup.

# What Changes

- Restore the exact pinned standard-age checkpoint and separately trusted head into
  an absent root through a bounded private offline verification/quarantine path.
- Persist new installation identity and immutable provenance while retaining the
  exact historical successor journal lineage and committed content/receipts.
- Require fresh operator enrollment and revoke all old sessions/grants/pending work.
- Publish a complete private directory with durable no-replace atomic rename and
  concrete same-input retries after staged or post-publication interruption.
- Recheck initially absent/empty admission under the writer lock before source mutation.

# Impact

Integrated checkpoint19/restore20 follow artifact16/schedule17/account18. No dependency, provider call, off-host latestness, historical
export replay, in-place merge or source unseal. Combined independent review and
integrated Linux acceptance remain publication gates.
