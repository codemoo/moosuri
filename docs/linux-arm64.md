# Linux ARM64 development build

The current assistant, independent vault service and experimental credential companion
cross-compile for `aarch64-unknown-linux-gnu`. The assistant and vault include their
React assets. A synthetic run under QEMU user-mode emulation confirmed initialization,
authenticated HTTP, SQLite persistence, session continuity after restart, deletion,
writer locking and the separate vault's public shell/authentication boundary.
See the [recorded environment and results](reviews/2026-11-15-linux-arm64.md).

This is a development-platform result. Official signed ARM64 packages, native ARM
hardware acceptance, nested installer/update execution, Chromium, provider logins,
actual companion pairing and performance measurements remain open. The tested GNU
runtime used an ARM glibc2.35 sysroot; this does not establish support for every Linux
distribution or a musl/static binary.

## Build

On an ARM64 Linux machine, follow the ordinary [development build](development.md)
and the [separate vault build](vault-service-client.md). On an x86_64 Linux build
machine, provide a matching AArch64 GNU C compiler, binutils and target sysroot, then
install the matching Rust standard library. For the tested Rust version:

```sh
rustup target add --toolchain 1.89.0 aarch64-unknown-linux-gnu
```

Build the assistant React app and vault React/WASM app first using their documented
locked dependencies. Then cross-compile the three native programs:

```sh
CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc \
CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc \
AR_aarch64_unknown_linux_gnu=aarch64-linux-gnu-ar \
cargo +1.89.0 build --locked --target aarch64-unknown-linux-gnu \
  -p moosuri -p moosuri-vault-service -p moosuri-credential-broker
```

The binaries are in `target/aarch64-unknown-linux-gnu/debug/`. Native Linux users
can run them directly. For an isolated cross-build check, an explicit QEMU executable
can start a selected binary without registering a host binfmt interpreter:

```sh
qemu-aarch64-static -L /path/to/arm64-sysroot \
  target/aarch64-unknown-linux-gnu/debug/moosuri installation probe
```

Use a fresh private data directory and synthetic accounts for `init`, `serve` and
`doctor`. An application that starts another ARM executable needs native execution
or a separately configured interpreter path; the direct QEMU check does not establish
that behavior. Build tools are not runtime dependencies of the resulting native service.

The validation environment mounted existing compiler/sysroot files read-only and
used hash-checked Rust target libraries and an extracted QEMU executable in temporary
storage. No host packages, kernel/binfmt registration, operating services or CI were
changed. Emulator binaries and generated artifacts are not included in this repository.
