# moosuri web

The React/TypeScript account foundation. `/app` opens the signed-in user's
conversation space; `/admin` provides account metadata and local invitations
for administrators. Both routes use the same bundle and same-origin API.

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

Implemented: session restoration, local sign-in, required password change,
sign-out, account-scoped empty conversation list/create/delete, administrator
user list/create, and one-time temporary password display/copy. Each private
workspace owns an abortable request lifetime; closing a session discards late
results and unmounts private state. Network failures do not imply a 401.

Message sending, model connections, setup wizard, delivery email, vault,
connectors, attachment/voice input, and PWA installation/offline support are
not implemented. The composer is disabled and no assistant output is fabricated.
There is no service worker, private browser cache, local credential storage,
or live CI workflow. The operator initializes the server with `moosuri init`.

Tests use synthetic accounts and mocked same-origin HTTP responses. They cover
restricted login, session failure, late account responses, member/admin
separation, failed deletion, server-confirmed mutations, and invitation cleanup.
They do not claim real-device, production, provider or email verification.
