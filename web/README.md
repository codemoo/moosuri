# moosuri web

The React/TypeScript web and PWA client. `/app` opens the signed-in user's saved
conversation space; `/admin` manages accounts and installation settings. Both use
the same bundle and same-origin Rust API. See [current support](../docs/status.md)
for implemented features and remaining work.

Requires Node.js 22.22+ and npm. From this directory:

```sh
npm ci
npm run typecheck
npm test
npm run build
```

The build is emitted to `dist/` for the Rust server. `npm run dev` serves the UI
on `127.0.0.1:5173` and proxies `/api` to the Rust server on `127.0.0.1:3210`.
For this workflow, start Rust with `--origin http://127.0.0.1:5173 --dev` so its
exact Host/Origin checks match the browser; the proxy preserves Host.
`shared/api-types.ts` is generated from `moosuri-core` using `moosuri export-types`;
do not independently edit the contract.

Each private workspace owns an abortable request lifetime; session changes discard
late results and unmount private state. Network failure is distinct from401.
[Chat](../docs/chat.md) covers saved streaming conversations and bounded same-tab
draft/pending-request recovery after exact session and thread-list confirmation.
Recovery never automatically posts a message. [PWA](../docs/pwa.md) documents the
public-only offline shell; private API responses and media are not cached there.
The independent vault client lives in `vault-web/`.

Initialize the Rust server with `moosuri init`. CI stays disabled. Component tests
use synthetic accounts and mocked same-origin responses. Actual Rust/SQLite/browser
acceptance is recorded separately in [the dated reviews](../docs/status.md#검증-기록을-읽는-기준);
neither proves real-device or live-provider support. Microphone/STT and incoming PWA
share remain separate implementation work.
