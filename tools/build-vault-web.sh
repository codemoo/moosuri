#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
case "$(wasm-bindgen --version)" in
  'wasm-bindgen 0.2.130') ;;
  *) echo 'wasm_bindgen_0_2_130_required' >&2; exit 1;;
esac
# The core receives entropy only from the Worker CSPRNG import. Native entropy
# remains disabled for the selected WASM package. Bound the entire WASM memory.
RUSTFLAGS='-C link-arg=--max-memory=402653184' cargo build --locked --release --target wasm32-unknown-unknown -p moosuri-vault-wasm
wasm-bindgen --target web --out-dir vault-web/src/wasm --out-name vault_crypto "${CARGO_TARGET_DIR:-target}/wasm32-unknown-unknown/release/moosuri_vault_wasm.wasm"
cd vault-web
npm ci --ignore-scripts
npm run build
