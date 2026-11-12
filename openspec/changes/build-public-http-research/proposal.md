# Change: durable public HTTP research

## Why
The assistant needs actual public-page reading and configured search while keeping
tenant scope, durable retries, bounded execution and final explicit mail approval.

## What changes
- Add typed web_read/web_search through persisted asynchronous external slots.
- Pin actual operator capabilities in version3 while retaining profiles1/2.
- Show server-observed current-run source URLs, time and coverage limits in React.
- Add owner-only optional search-key configuration; keep HTTP and browser claims distinct.

## Impact
Rust core/store/agent/server/CLI and React evidence; no browser automation, arbitrary
network tool, new approval pipeline or mail send authority. Existing execution and
isolation requirements R/F remain in force. See docs/web-research.md for limits.
