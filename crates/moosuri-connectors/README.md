# moosuri-connectors

The first module is a bounded text model protocol/HTTP transport foundation.
See [the contract and limits](../../docs/model-transport.md). Admin model discovery
and synthetic verification now use this library through [provider settings](../../docs/provider-settings.md).
OAuth and durable conversation runs remain separate work. All current tests use
synthetic data and loopback HTTP.

```sh
cargo test --locked -p moosuri-connectors
cargo clippy --locked -p moosuri-connectors --all-targets -- -D warnings
```
