# Source-only build of matched host services and member runtime. No developer
# toolchain, prebuilt web assets, credentials, or local GWS configuration enters.
FROM rust:1.89.0-bookworm@sha256:948f9b08a66e7fe01b03a98ef1c7568292e07ec2e4fe90d88c07bb14563c84ff AS source
ENV RUSTUP_TOOLCHAIN=1.89.0
RUN apt-get update && apt-get install -y --no-install-recommends cmake \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /source
COPY Cargo.toml Cargo.lock rust-toolchain.toml LICENSE-APACHE LICENSE-MIT ./
COPY apps ./apps
COPY crates ./crates
COPY tools ./tools
COPY community ./community
COPY migrations ./migrations
COPY deployment ./deployment
COPY web/public/moosuri-icon-192.png ./web/public/

FROM source AS bundle-tool-build
RUN cargo build --locked --release -p xtask

FROM scratch AS bundle-tool
COPY --from=bundle-tool-build /source/target/release/xtask /

FROM source AS wasm
RUN rustup target add wasm32-unknown-unknown \
    && cargo install --locked wasm-bindgen-cli --version 0.2.130
RUN RUSTFLAGS='-C link-arg=--max-memory=402653184' cargo build --locked --release \
      --target wasm32-unknown-unknown -p moosuri-vault-wasm \
    && wasm-bindgen --target web --out-dir /wasm --out-name vault_crypto \
      target/wasm32-unknown-unknown/release/moosuri_vault_wasm.wasm

FROM node:22-bookworm-slim@sha256:be979e5523c76fa4d5ca4ebd3099078f220aa47ec39c48ad5b5df9314debd6eb AS web
WORKDIR /source
COPY web ./web
COPY vault-web ./vault-web
COPY --from=wasm /wasm ./vault-web/src/wasm
RUN npm ci --ignore-scripts --prefix web && npm run build --prefix web \
    && npm ci --ignore-scripts --prefix vault-web && npm run build --prefix vault-web

FROM source AS native
COPY --from=web /source/web/dist ./web/dist
COPY --from=web /source/vault-web/dist ./vault-web/dist
ARG CARGO_BUILD_JOBS=2
RUN cargo build --locked --release -p moosuri -p moosuri-vault-service -p xtask \
    && cargo build --locked --release -p moosuri-connectors --example browser_worker --example browser_proxy \
    && test -s web/dist/index.html && test -s vault-web/dist/index.html \
    && mkdir /artifacts \
    && cp target/release/moosuri target/release/moosuri-vault-service target/release/xtask \
      target/release/examples/browser_worker target/release/examples/browser_proxy /artifacts/ \
    && strip --strip-debug /artifacts/* \
    && cd /artifacts && sha256sum * > SHA256SUMS

FROM scratch AS artifacts
COPY --from=native /artifacts/ /
COPY LICENSE-APACHE LICENSE-MIT /
