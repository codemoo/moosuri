# Design

Use existing AES-GCM v2 fields/envelopes, Argon2id13/HKDF source unwrap and domain-separated HKDF/HMAC archive authentication. Exact bounds/policies precede KDF; manifest authenticates source header plus framed ordered count/items. Every payload authenticates before any preview/staged state becomes visible. Archive verification does not manufacture a HeaderReceipt.

Archive import fingerprints are client-only, sealed selections use new destination IDs/key/owner binding, and immutable per-item Put operations reuse exact partial/lost-ACK reconciliation. Lock interrupts the Worker even during synchronous source KDF. Fresh export inventory is pinned and rechecked. The [protocol](../../../docs/vault-archive.md) defines format, lifetime, visible metadata and limitations. Historical source wrappers survive later protection changes; no rollback freshness or signed-JavaScript claim is made.
