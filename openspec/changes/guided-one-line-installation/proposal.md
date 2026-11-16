# Guided one-line preview installation

## Why
The current signed installer requires manually choosing paths, creating an account,
writing a service and starting the dashboard. New users need one terminal command
and an understandable English/Korean setup on macOS, Linux and WSL2.

## What changes
- Publish target-pinned preview bootstrap archives and a minimal HTTPS shell entry point.
- Add a bilingual Rust wizard with arrow/number selection, safe resume, administrator
  creation, optional authenticated API/local model setup and subscription guidance.
- Add managed local access and exact user-owned launchd/systemd lifecycle commands.
- Preserve signed update/recovery, SQLite/account isolation, private credentials,
  separate vault installation and disabled CI.

## Impact
R-001 / installation and onboarding; existing low-level installer remains available.
This preview does not imply stable-release, native Windows or live-provider acceptance.
