FROM rust:1.89.0-bookworm AS build
RUN apt-get update && apt-get install -y --no-install-recommends cmake && rm -rf /var/lib/apt/lists/*
WORKDIR /source
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates ./crates
COPY apps ./apps
COPY tools ./tools
RUN cargo build --locked --release -p moosuri-connectors --example browser_worker --example browser_proxy

FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends chromium ca-certificates fonts-liberation iptables iproute2 util-linux tini && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid 10001 browser && useradd --uid 10001 --gid 10001 --no-create-home --shell /usr/sbin/nologin browser
COPY --from=build /source/target/release/examples/browser_worker /usr/local/bin/browser-worker
COPY --from=build /source/target/release/examples/browser_proxy /usr/local/bin/browser-proxy
COPY deployment/browser/entrypoint.sh /capsule/entrypoint.sh
RUN chmod 0555 /capsule/entrypoint.sh
ENTRYPOINT ["/usr/bin/tini", "--", "/capsule/entrypoint.sh"]

FROM runtime AS test
RUN apt-get update && apt-get install -y --no-install-recommends python3 && rm -rf /var/lib/apt/lists/*
COPY deployment/browser/fixture.py deployment/browser/proxy-test.sh deployment/browser/loopback-canary.py /capsule/
# Test wrapper/canary exist only in this target. Production has neither Python
# nor a network-probe mode, and the public model protocol is unchanged.
RUN mv /usr/local/bin/browser-worker /usr/local/bin/browser-worker-real
COPY deployment/browser/worker-test.py /usr/local/bin/browser-worker
RUN chmod 0555 /capsule/proxy-test.sh /usr/local/bin/browser-worker \
    && chmod 0444 /capsule/fixture.py /capsule/loopback-canary.py
