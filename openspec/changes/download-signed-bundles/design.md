# Design

Keep transport inside installation/download.rs with the existing synchronous dispatcher;
a separate current-thread Tokio runtime owns the bounded reqwest operation. Reuse the
existing pinned manifest validation and complete local bundle/evidence verifier. No new
dependencies or binary execution are required.

Reject credentials/query/fragment/traversal before URL normalization. Accept only HTTPS
literal version paths, disable ambient proxies, reject all redirects and require HTTP200
identity responses. Every request is a fixed basename under the explicit base. Authenticate
manifest/signature before parsing, check signed provenance before following its fixed inventory
reference, then download/hash the binary and repeat the full local bundle contract. Normal
TLS trust may include one explicit bounded private CA; hostname validation is always required.

Only new private sibling staging files are mutation targets. Cleanup compares directory/file
identities and never recurses. Linux renameat2(RENAME_NOREPLACE) or macOS renamex_np(RENAME_EXCL)
publishes a fully fsynced bundle; an existing destination is never replaced. Ordinary errors
clean owned staging; abrupt kill/power loss may leave an unreferenced staging directory for
manual inspection. A post-rename parent sync error preserves the authenticated destination.

The synthetic TLS fixture is a Python stdlib server with an OpenSSL-generated CA and separate
leaf certificate. The production CLI uses its ordinary optional CA input. No HTTP exception,
TLS-validation bypass or test-only production transport hook is introduced.
