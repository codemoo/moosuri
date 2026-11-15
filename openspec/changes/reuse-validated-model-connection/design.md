# Design

Each invocation resolves every bounded address and checks current network policy
before consulting a per-ModelClient HTTP cache. The key contains origin and sorted
deduplicated socket addresses. An unchanged set reuses the reqwest Client with one
idle socket and a30second idle limit. New limits clear the cache. The client pool
stores no default authorization header; each request receives the existing scoped
header. No cache spans ModelClient instances or owners. Invalid answers are denied
even when a valid cached socket already exists. Changed answers replace the pool.

The existing explicit never-retry policy and cancellation/clean EOF behavior are
unchanged. Synthetic tests inspect accepted peer sockets rather than elapsed-time
thresholds. Live-provider latency savings depend on handshake cost and caller
client lifetime and are not inferred from loopback timing.
