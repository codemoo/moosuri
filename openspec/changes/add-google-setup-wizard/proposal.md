# Why

Self-hosted installers currently need to discover the Google web-client format,
callback URI and server option themselves. R04/R09/R10 and F01/F34/F56 require a
clear setup path while keeping each member's Gmail consent and data independent.

# What Changes

- Add a React administrator wizard for Google Cloud preparation, exact callback
  copying, downloaded web-client JSON validation/import and personal connection checks.
- Store the installation's app secret encrypted under the existing server-managed
  key; support immediate availability and restart persistence without a central broker.
- Preserve the explicit operator-file override, personal OAuth state/session binding,
  source-sync grants and mandatory approval before every email send.
- Fence setup edits, pending authorization, refresh results/errors and affected
  source runs. A local file check is not proof of Google API or consent readiness.

# Impact

Rust connectors/core/store/agent/server/CLI, SQLite migration10 and React admin/mail
navigation. Google app creation remains an installer action in Google Cloud. No
central authentication service, automatic account sharing, invitation delivery,
real-account setup or production deployment is included.
