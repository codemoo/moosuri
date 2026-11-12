# Model transport verification — 2026-11-12

This unit adds a Rust library for text protocol/HTTP transport. It does not wire
model profiles or actual chat into the application. All inputs, credentials and
provider responses in the checks are synthetic; network tests use loopback.

## Evidence

The integrated connector suite passes 43 tests: 32 protocol cases and 11 HTTP
boundary cases. The final workspace passes **60 tests** (the previous17 plus43)
on macOS and isolated Linux x86_64 with Rust1.89.0. Format and workspace/all-target
Clippy with warnings denied pass. Linux used the same pinned Rust image and
resource-limited container boundary recorded in the account unit; only public
source, generated public web assets and synthetic runtime inputs were mounted.
React code and generated DTOs are unchanged in this unit; the
preceding [account foundation](2026-11-12-account-foundation.md) remains its web
verification record.

Protocol fixtures cover all four dialects, fragmented UTF-8/CRLF/bare CR/multiline
SSE, late assistant phase, reasoning separation, cumulative/unknown usage,
refusal/incomplete/continuation outcomes, missing terminal markers, streamed
errors and size/event/output limits. Responses final snapshots are checked
against accumulated text/refusal content, not accepted solely by event name.

Real HTTP tests exercise hosted request headers and requested effort, exact
OpenRouter routing controls, compatible streaming, coalesced partial output then
failure, redirects without credential forwarding, no retry after 429, invalid
capability before dispatch, metadata/unregistered destination rejection,
pre/in-flight cancellation, bounded backpressure, closed consumers while headers
or body are held, response type and byte limits.

## Independent review

The transport review identified missing denial of Alibaba's metadata address and
receiver closure not interrupting stalled HTTP work. Both were fixed and narrowly
reviewed again, including IPv4-mapped IPv6 and held-header/body regressions.

The protocol review identified coalesced-frame errors dropping earlier events
and final Responses snapshots not being reconciled. The decoder now returns a
`DecodeBatch` containing valid prefix events plus a later error, removes completion
from a failed batch and rejects future decoding after a failure. Failing-frame
events are discarded. Bounded part aggregates are reconciled at all done/final
snapshots; a refusal part remains a refusal even when its text is empty. The
reviewer inspected the repaired source and new focused fixtures and closed both
findings. Independent reviews were read-only; main ran the integrated suites.

## Dependencies and limits

The 224 registry packages in this unit's Cargo.lock were queried through OSV on
2026-11-12. No advisory/pagination entries were returned. Reqwest 0.13.5 uses
rustls and AWS-LC in this build; native crypto dependencies are not represented as
pure Rust. Lock/advisory checks do not prove absence of undiscovered issues.

No real provider availability, TLS connection to a live provider, account-specific
model/effort capability, subscription login, token billing, remote cancellation,
mission quality or external action is established here. DNS pinning and TLS
configuration were inspected; test servers use explicitly authorized loopback
HTTP. Actual OAuth/probe/cost checks belong to later connection/run integration.
No production deployment or CI run was performed.
