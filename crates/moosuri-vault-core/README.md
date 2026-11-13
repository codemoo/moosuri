# moosuri-vault-core

Client-only native Rust cryptography, TOTP and bounded synthetic Apple CSV import. No service, model tool, browser/WASM bundle or credential broker is implemented.

Read the [core guide](../../docs/vault-core.md), [design](../../docs/vault-core-design.md) and [dependency review](../../docs/vault-core-dependencies.md) before integrating. Native verification: `cargo test --locked -p moosuri-vault-core` and `cargo clippy --locked -p moosuri-vault-core --all-targets -- -D warnings`.
