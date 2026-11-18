# Build with the repository root as the context. Use the resulting local image
# config ID (sha256:...) when provisioning members.
FROM rust:1.89.0-bookworm@sha256:948f9b08a66e7fe01b03a98ef1c7568292e07ec2e4fe90d88c07bb14563c84ff AS build
RUN apt-get update \
    && apt-get install -y --no-install-recommends cmake \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /source
COPY Cargo.toml Cargo.lock rust-toolchain.toml LICENSE-APACHE LICENSE-MIT ./
COPY apps ./apps
COPY crates ./crates
COPY tools ./tools
COPY community ./community
COPY migrations ./migrations
COPY web/public/moosuri-icon-192.png ./web/public/
ARG CARGO_BUILD_JOBS=4
RUN RUSTUP_TOOLCHAIN=1.89.0 cargo build --locked --release -p moosuri

FROM debian:bookworm-slim@sha256:734a72b3d6142a71993b4bc580ca01d8ca687f77a2c422d96a961d3376b179bc AS gws
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /release
# Checksums from the official googleworkspace/cli v0.22.0 release assets.
RUN set -eu; \
    case "$(uname -m)" in \
        x86_64) target=x86_64-unknown-linux-musl; digest=269cf88d6b6a2543b459bbb973784acce3693fa0a436d740093fb00ebf54bd22 ;; \
        aarch64) target=aarch64-unknown-linux-musl; digest=176a8ac419bdc1401d86a1e52ad08c158fb439584554ed1a254686328e704e20 ;; \
        *) echo 'Kiri member image supports Linux amd64 and arm64' >&2; exit 1 ;; \
    esac; \
    asset="google-workspace-cli-$target"; \
    curl -fLSs "https://github.com/googleworkspace/cli/releases/download/v0.22.0/$asset.tar.gz" -o gws.tar.gz; \
    printf '%s  %s\n' "$digest" gws.tar.gz | sha256sum -c -; \
    tar -xzf gws.tar.gz --strip-components=1 "$asset/gws" "$asset/LICENSE"; \
    version="$(./gws --version)"; \
    test "$(printf '%s\n' "$version" | sed -n '1p')" = 'gws 0.22.0'; \
    discovery_source='https://raw.githubusercontent.com/googleapis/google-api-go-client/a1b2b18eba794c7d9f8e86b7db1d12a69c081002'; \
    curl -fLSs "$discovery_source/gmail/v1/gmail-api.json" -o gmail-v1.json; \
    curl -fLSs "$discovery_source/LICENSE" -o gmail-discovery-LICENSE; \
    printf '%s  %s\n' \
        839b7963df95481f3cd2ba7aa1e906f1fc67c4ae89719d09bb7305040e99f0fb gmail-v1.json \
        110244b02140866ee37d17fa7449436a377ec3b85a481fbb208f4c87964382de gmail-discovery-LICENSE \
        | sha256sum -c -; \
    chmod 0555 gws; \
    chmod 0444 LICENSE gmail-v1.json gmail-discovery-LICENSE

FROM debian:bookworm-slim@sha256:734a72b3d6142a71993b4bc580ca01d8ca687f77a2c422d96a961d3376b179bc
COPY --from=gws /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
COPY --from=gws /release/gws /usr/local/bin/gws
COPY --from=gws /release/LICENSE /usr/share/doc/google-workspace-cli/copyright
COPY --from=gws /release/gmail-v1.json /usr/local/share/moosuri/gmail-v1.json
COPY --from=gws /release/gmail-discovery-LICENSE /usr/share/doc/google-gmail-discovery/copyright
COPY --from=build /source/target/release/moosuri /usr/local/bin/moosuri
COPY LICENSE-APACHE LICENSE-MIT /usr/share/doc/moosuri/
COPY deployment/kiri/text-preview.json /etc/moosuri/text-preview.json
RUN mkdir -p /data /ipc /google /model \
    && chmod 0555 /usr/local/bin/moosuri \
    && chmod 0444 /etc/moosuri/text-preview.json \
    && /usr/local/bin/moosuri --version
ENTRYPOINT ["/usr/local/bin/moosuri"]
