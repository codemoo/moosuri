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
RUN apt-get update && apt-get install -y --no-install-recommends python3 openssl libnss3-tools && rm -rf /var/lib/apt/lists/*
# This generated CA/key pair belongs only to the test image and owned CONNECT
# fixture. Production trust, Chromium flags and proxy policy remain unchanged.
RUN mkdir -p /capsule/test-tls \
    && openssl req -x509 -newkey rsa:2048 -nodes -days 3650 -subj '/CN=Moosuri Synthetic Test CA' -keyout /capsule/test-tls/ca.key -out /capsule/test-tls/ca.crt >/dev/null 2>&1 \
    && openssl req -newkey rsa:2048 -nodes -subj '/CN=fixture.example.com' -keyout /capsule/test-tls/server.key -out /capsule/test-tls/server.csr >/dev/null 2>&1 \
    && printf '%s\n' 'subjectAltName=DNS:fixture.example.com,IP:93.184.216.34' 'basicConstraints=critical,CA:FALSE' 'keyUsage=critical,digitalSignature,keyEncipherment' 'extendedKeyUsage=serverAuth' > /capsule/test-tls/server.ext \
    && openssl x509 -req -in /capsule/test-tls/server.csr -CA /capsule/test-tls/ca.crt -CAkey /capsule/test-tls/ca.key -CAcreateserial -days 3650 -extfile /capsule/test-tls/server.ext -out /capsule/test-tls/server.crt >/dev/null 2>&1 \
    && rm /capsule/test-tls/ca.key /capsule/test-tls/server.csr /capsule/test-tls/server.ext /capsule/test-tls/ca.srl \
    && chmod 0400 /capsule/test-tls/server.key \
    && cp /capsule/test-tls/ca.crt /usr/local/share/ca-certificates/moosuri-synthetic-fixture.crt \
    && update-ca-certificates >/dev/null 2>&1 \
    && mv /usr/bin/chromium /usr/bin/chromium-real
COPY deployment/browser/chromium-test.sh /usr/bin/chromium
RUN chmod 0555 /usr/bin/chromium
COPY deployment/browser/fixture.py deployment/browser/proxy-test.sh deployment/browser/loopback-canary.py /capsule/
# Test wrapper/canary exist only in this target. Production has neither Python
# nor a network-probe mode, and the public model protocol is unchanged.
RUN mv /usr/local/bin/browser-worker /usr/local/bin/browser-worker-real
COPY deployment/browser/worker-test.py /usr/local/bin/browser-worker
RUN chmod 0555 /capsule/proxy-test.sh /usr/local/bin/browser-worker \
    && chmod 0444 /capsule/fixture.py /capsule/loopback-canary.py
