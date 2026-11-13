# Why

R29 removes Kakao collection from the public distribution. The owner will run that
collector privately and connect it through the same custom protocol available to
other users. Official mail/Slack adapters and external producers need shared
provenance, sync state and receipt tracking rather than personal collector code.

# What Changes

- Keep F38 as a private, excluded integration boundary; add F70 for public official/custom ingestion.
- Define a shared connection/run/batch/provenance contract and a narrow authenticated HTTP push path.
- Implement opt-in Gmail/Slack sync through the same commit boundary; distinguish category from readiness.
- Track scope, target code and new acceptance cases in the existing audit/xtask workflow.

# Impact

Rust core/store/connectors/server/agent, React connections and knowledge views,
new SQLite migration, docs and audit mappings. Current manual ingestion and Gmail
interactive tools remain supported. No Kakao executable, installer or adapter is
shipped; no private collector, cron, DB, deployment or email-send policy is changed.
Implementation and synthetic SQLite/HTTP/React/Linux checks are recorded in
[the acceptance record](../../../docs/reviews/2026-11-13-source-connections.md).
Live-provider acceptance remains separate and unchecked; no production setup is implied.
