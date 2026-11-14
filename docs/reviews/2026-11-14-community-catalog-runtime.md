# Public catalog discovery in Suggestions

2026-11-14. Accepted bounded development-preview implementation, with synthetic
public transport and isolated accounts. This is not a live registry publication or
proof of a publisher's safety, rights or task success.

## Delivered behavior

An operator can configure one public GitHub repository, simple ref and directory
containing the existing static catalog artifacts. Opening Suggestions checks only
the index, at a daily interval with ETag; manual attempts have a one-minute minimum
interval. It does not send private queries or invoke a model. No live catalog is
configured by default.

Only a selected package is fetched. Its artifact digest and strict metadata must
match the index, and the existing GitHub adapter must independently verify the
original exact commit/tree/blob bytes before staging. Artifact assertions alone
cannot create verified source provenance. An already verified immutable snapshot
can be reused. Preview does not install; installation does not start a task.

Schema35 persists the public cache and bounded refresh lease. Owner/session and
source/generation checks reject stale completion. Offline failures preserve the
last verified index and installations; corrupt cache is excluded and recovered
with an unconditional fetch. Preferences and active versions remain per user.
Restore clears the cache without weakening schema34 helper-session restrictions.

## Independent review and repairs

The first review verified the frozen900-file worker and found two medium defects:

- Preview of a new version used that version's requirements to render the still
  installed version. This could remove a valid Start action or falsely advertise
  readiness. Both catalog preview paths now use active-installation readiness;
  the independent GitHub importer retains its separate candidate-preview contract.
- Initial/plain refresh did not fetch structured catalog status, hiding failures
  without cached offers and retaining obsolete manual status. Status is now read
  after the cards request under the same account/generation/abort fence. Existing
  cards survive a status-only error.

Real Rust and React tests reproduced these failures before repair. The repaired
tests retain installed readiness in both capability-change directions and cover
empty unavailable/corrupt indexes, recovery, and late old-account status. Independent
rereview verified all901 integrated files and the six-path repair delta without
another material finding. No provenance, authority or restore finding remained.

## Executed verification

The initial integrated Linux build passed Store413, agent97, connectors187,
core3 and HTTP40 tests, strict affected-package Clippy and production build. The
two reference-Go-age Store checks, one live-account agent check and one live public
GitHub HTTP check remain explicitly ignored. A nested Store child result is not
an additional unique test.

After the review repairs, the exact repaired source passed Linux community16,
the composed catalog HTTP test, strict affected-package Clippy and production
build. The HTTP fixture serves a valid artifact while denying original acquisition:
preview fails. Enabling exact original acquisition permits preview and then a
separate explicit install. Verified cache reuse performs no further origin fetch.

Native checks passed the repaired community16 tests. The integrated native default
build, generated DTO equality and normal/build feature graph without test-fixtures
were checked before the two UI/readiness repairs; the exact repaired Linux
production build is recorded above. The
full web suite passed384 before the repairs; the four affected web modules then
passed20 tests on the repaired source, together with both TypeScript checks and
production build. These overlapping counts are not summed. Formatting and strict
OpenSpec were checked; their initial formatting/scenario omissions were repaired
without changing runtime behavior.

Actual Chrome used the production React build, real Rust router/SQLite and an
explicit test-fixture executable with separate loopback public transports. It
verified index-only discovery, denied-origin preview, successful original-source
verification, explicit installation without a chat run, member/owner installation
isolation, retained installation during offline refresh and conditional304 recovery.
The actual one-minute cooldown was retained. The390px viewport had no horizontal
overflow and its screenshot was inspected. The fixture recorded3 index requests,
2 artifact requests and8 original-source requests, including the deliberate
failure; these are fixture counters, not a global network audit. Both servers and
Chrome stopped cleanly, and the disposable binary/profile were removed.

## Limits

No real catalog was published or enabled, and live GitHub availability, physical
phones, production DNS/TLS and a complete crash matrix were not tested. Package
signatures, publisher attestations, revocation manifests, cache GC, private GitHub,
arbitrary hosts/refs, learned-skill export and automatic activation remain separate
work. Existing mail final-preview approval and native capability limits are unchanged.

[Configuration and user behavior](../community-skills.md) ·
[Current support](../status.md)
