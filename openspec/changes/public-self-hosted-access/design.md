# Design

The user owns DNS and inbound routing. The installer checks DNS and temporarily
serves only a random challenge path with an independent random response. A bounded
HTTPS probe pins the resolved public addresses, verifies normal TLS and rejects
redirects/wrong bodies. Only successful verification activates the application origin.
Native mode uses a pinned Rust ACME TLS-ALPN-01 listener; proxy mode exposes the
challenge on the selected loopback backend. No assistant data is served during
verification. Public checks apply only to managed public receipts, preserving
older LAN/private HTTPS installations.

Certificate renewal runs with the server. Private atomic account/certificate
caches live outside the Store root, survive runtime removal and are not part of
Store backups. Native mode uses IPv4; existing proxies support other topologies.
The receipt binds installation identity, original/target origin and exact generated
examples. Partial transitions block start/update and support connect/disconnect
recovery. Application login, Secure cookies and exact Origin checks remain.

No DNS API, token input, record generation, router/firewall rewrite, sudo, automatic
tunnel or third-party proxy configuration is provided. A same-host probe does not
prove all external-network reachability; TLS-ALPN additionally requires CA access
on external443. Existing listeners and services are preserved.
