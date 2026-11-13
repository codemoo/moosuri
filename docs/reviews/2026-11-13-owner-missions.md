# Owner missions: implementation and acceptance

This bounded increment implements explicit one-time owner requests at `/app/missions`, with a plan, typed read tools, stored receipts, selected machine checks and a reviewable result. It does not complete recurring jobs, change-only comparisons, conversational task capture, authenticated browser/mail execution or semantic evaluation. All tests use synthetic data and isolated local installations. No real email was sent, no production service was deployed and CI remains disabled.

## Authority and integration

Assistant schema25 follows push23 and vault24. A mission has an immutable request, constraints, checks, provider/tools, account epoch and one-day admission expiry. The optional durable grant is distinct from a real session UserContext and admits only knowledge reads, instruction skills, exact calculation and configured public HTTP. Store admission rejects mail, authenticated browser, artifact writes, vault and arbitrary code; a model cannot widen that scope. The plan and at most two repairs share the original six-model/twelve-tool execution limits and runtime deadline. Account/source/skill/provider withdrawal, deletion journal and restored-installation quarantine fence old work.

The actual encrypted export/restore test confirms completed history remains readable and unfinished grants cannot run. A late migration25 failure rolls back and retries while retaining earlier authority tables. Original push/vault/media/schedule contracts and account lifetime hooks remain present. Public DTOs match Rust generation, and the Cargo lock is unchanged from the vault24 publication.

## Findings and repairs

- An acknowledged final proposal at exactly six model calls or twelve tool calls could not finish local verification after restart. Recovery now admits that exact stable proposal without new dispatch or expanded capacity; unfinished/unknown work remains ineligible.
- Cancel after a lost Resume acknowledgement could replay the earlier action. The UI now reconciles current state, retries only the same exact uncertain intent and issues a fresh current-revision cancellation when needed.
- Repeated controls at legacy chat attempt32 could return queued while the underlying run stayed cancelled. Stable mission continuation now retires the old lease in the same transaction and claims a new nonce at the boundary. Old run/step mutations fail while exact late accounting receipts remain valid. Forty-control and attempt32 restart cases pass; ordinary chat retry policy is unchanged.
- Typed calculation checks did not establish arbitrary prose conditions. Nonempty conditions now require explicit or implicit owner review, preserve the original request and settle partial without extra repair calls when all machine checks pass. Empty-condition exact calculations can pass their selected criteria.
- Rendered Chrome found an oversized checkbox and missing direct-route mapping. Mission scrolling/control sizing was corrected; `/app/missions` now serves the public shell on fresh load/reload without treating private API paths as static content.

Independent reviews covered the original authority/runtime/UI integration, the capacity/retry repairs, attempt32 lease repair and the eight-path conditions/UI change. No unresolved material finding remains in those reviewed scopes. Reviewer fixture execution is distinguished from root browser/Linux acceptance; these are not external security certifications.

## Verification

The initial integrated native suites passed Store291, agent62, server42 and CLI18 tests, with five explicitly opt-in checks ignored across those suites. Subsequent changed paths passed the33 mission-related Store cases, real synthetic model supervisor flow, mission HTTP API, React246 tests, TypeScript/build, workspace all-target clippy and fmt. Generated DTO and planning/OpenSpec checks are recorded separately from product outcomes. The new tests exercise user-visible failures and receipts rather than importing old private tests.

Linux x86_64 ran eight bounded phases on hash-verified source with network disabled: mission33, restore9, ordinary chat21, shared execution23, vault authority4, mission runtime1, server42 and CLI18, totaling151 passed with two opt-in tests ignored. The first Linux run exposed a synthetic backup-directory permission assumption; its key/head/destination directories now explicitly use0700 instead of depending on the shell umask. Product directory validation was not weakened. The final one-line deep-link mapping is additionally checked by the CLI suite.

Actual Chrome390×844 used the real Rust server, SQLite, model HTTP parser and a synthetic loopback provider. It covered exact decimal completion with an actual receipt, prose-condition partial result, pause during in-flight work with no unsafe resume, cancellation, draft preservation on navigation, same-session/server-restart history, direct deep links and deletion surviving reload. Header/own-scroll behavior, checkbox sizing and absence of horizontal overflow were inspected. Final application console had no error; the initial anonymous session probe401 is expected. A test script's relative APIRequest URL was corrected to an absolute URL; that harness error did not change product source.

These checks establish the stated bounded behavior. They do not measure live-model task success, mobile-device delivery, open-ended semantic correctness or whole-roadmap completion.
