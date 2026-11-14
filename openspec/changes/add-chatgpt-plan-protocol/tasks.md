## Implementation

- [x] Add explicit fixed-origin plan constructor and separate catalog/text dialect; verify exact synthetic request/catalog and loopback HTTP assertions.
- [x] Add fixed namespace tool encoder/decoder and private checkpoint dialect pinning; verify fragmented two-round/checkpoint identity and API regression fixtures.
- [x] Preserve safe known plan errors and partial usage/text observations; verify post-delta failures, wrong identity, held EOF and no completed event.
- [x] Add synthetic fragmentation, identity, catalog, request and loopback tests.
- [x] Run connector tests, clippy and formatting checks in the assigned shared validation slot: 153 connector tests passed before independent review, connector all-target clippy with warnings denied passed, rustfmt check passed; strict OpenSpec validation passed.
- [x] Document that OAuth/personal login/selection and media remain unavailable; review source diff for no core/store/UI/dependency changes.

## Dependent availability gates

- [x] Integrate owner-scoped OAuth/JWKS/refresh/revoke and bounded native VM lifecycle in add-personal-chatgpt and add-chatgpt-vm-handoff; live acceptance stays separate.
- [ ] Verify an actual consenting account's exact model/effort/text/tool capabilities in an authorized separate test connection.

## Independent review repair P2-1

- [x] Require established explicit response identity before preserving failed-frame plan usage; validate all full progress/queued snapshot identities without changing ordinary API behavior.
- [x] Add decoder and real loopback regression cases for pre-created/missing/null/foreign identity, valid attributed failed usage, prior visible text and foreign progress before a valid terminal/tool round.
- [x] Run repaired-source connector tests/clippy/fmt/OpenSpec gates in the reserved next native slot: exact source-input mtimes refreshed without byte changes; 158 connector tests and all-target clippy with warnings denied passed; formatting and strict OpenSpec passed. Original freeze/logs preserved separately.
- [x] Independent repair rereview accepted P2-1 without new material findings; root integrated Linux connectors159, related Store/agent/CLI checks and affected six-crate Clippy passed.
