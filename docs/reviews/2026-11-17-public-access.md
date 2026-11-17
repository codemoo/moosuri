# Public self-hosted access · 2026-11-17

The installer offers Tailscale or a user-owned public domain, retaining local setup
for deferred networking. Native Rust HTTPS renews certificates with a private
atomic cache; existing proxies and manually configured DNS remain available.
DNS is user-managed. Public access requires read-only DNS resolution and an
independent one-use HTTPS response from this server before origin activation.

Independent review found certificate files inside the Store root invalidating
its closed allowlist, and public DNS requirements being applied to earlier private
HTTPS proxies. Keys now use an independently private sibling; public browser checks
require a managed public receipt. A second independent source review of the
registered-domain flow found no remaining material blockers.

## Verified outcomes

- Mac ARM64: 32 installation contracts, all 12 guided-terminal scenarios and
  all-target Clippy passed. Linux x64: 31 applicable contracts, the same 12
  terminal scenarios and Clippy passed. The one Mac-only contract checks ACLs.
- A synthetic, normally verified HTTPS connection proves this server's independent
  one-use response before origin activation. Wrong destinations, reflection and
  foreign installation receipts cannot activate a domain. An existing HTTPS proxy
  must route the temporary challenge to this installation's loopback backend.
- Interrupted file/origin publication resumes or restores the exact earlier
  installation. Pending transitions block ordinary startup. The native HTTPS app
  accepts a synthetic account login, sets Secure cookies, rejects the wrong Origin
  and ignores spoofed forwarding headers. Disconnect reopens the original local
  app and retains account data and certificate keys.
- Cached test certificates serve real TLS across restart; untrusted certificates
  and symlink cache replacement fail. Keys are private and outside Store backups.
- Linux x64 and ARM64 release builds passed from the exact reviewed source
  snapshot. Mac release packaging and public installer acceptance remain pending.

Tests use isolated homes, synthetic accounts and a cached self-signed test CA;
the loopback address/CA overrides are compiled only with `test-fixtures`.
Earlier concurrent Mac build/test attempts caused terminal prompt timeouts; the
complete final suite passed sequentially without competing release builds.

Verification does not change a real DNS zone, account, credential or certificate
provider. Real ACME issuance, phone access, router/firewall and physical-platform
acceptance remain separate. A check from the server itself does not prove
reachability from every external network; NAT hairpin limitations are documented.
CI remains disabled.
