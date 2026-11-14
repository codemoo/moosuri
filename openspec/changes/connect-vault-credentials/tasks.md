## 1. Define
- [x] 1.1 Specify scoped binding, automatic reuse, trust limits and failure scenarios.

## 2. Implement and validate
- [x] 2.1 Select and independently review device pairing, request authentication and receipt protocol. The default-off foundation is implemented and tested; actual adapters and user flows remain below.
- [ ] 2.2 Implement opaque bindings and first-use/ambiguous-account UX without repeated routine approvals.
- [ ] 2.3 Add typed API/Google credential adapters with no secret-bearing tool results.
- [ ] 2.4 Implement locked/offline resume, rotation/revoke and broker-side effect reconciliation; verify two-broker lost-ACK/late-owner cases without failover sends.
- [ ] 2.5 Enforce device-local grant ceilings and online commitment/revocation ordering; pass paired-server forgery and held-request race cases in CRED02/CRED03.
- [ ] 2.6 Prove CRED secret-containment/isolation/replay cases with synthetic credentials and document supported execution modes; ship supported broker paths as a v1 opt-in package.
