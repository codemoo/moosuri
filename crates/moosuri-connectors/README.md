# moosuri-connectors

The first module is a bounded text model protocol/HTTP transport foundation.
See [the contract and limits](../../docs/model-transport.md). The web assistant
is not connected to this library yet; OAuth, model probes, key storage and durable
runs are separate work. All current tests use synthetic data and loopback HTTP.

```sh
cargo test --locked -p moosuri-connectors
cargo clippy --locked -p moosuri-connectors --all-targets -- -D warnings
```
