# Native local browser session groundwork

Implemented a local-human-only Chromium launcher in the existing trusted companion.
It owns an isolated temporary incognito profile and inherited CDP pipe, keeps one
bounded session in the existing private journal, and emits only fixed lifecycle
metadata and page count. There is no server/model command, credential use, page
content export, site login/read adapter, or arbitrary browser effect API.

Focused native checks on macOS with Rust1.89:

- `cargo test --locked --offline -p moosuri-credential-broker local_browser -- --test-threads=1`:
  three passed; the explicit real-browser test was initially ignored. These exercise
  actual Engine/journal/pipe handling, secret-bearing synthetic target metadata,
  confirmation/idle polling, expiry/clock rollback, revoke, recovery and unknown
  old process ownership. They do not emulate a live website.
- `cargo clippy --locked --offline -p moosuri-credential-broker --all-targets -- -D warnings`:
  passed after removing one needless return binding.
- Explicit `actual_local_chromium_pipe_blank_window_and_cleanup --ignored` with the
  installed Google Chrome executable: one passed. It started one new blank window,
  counted its page through the real inherited pipe, confirmed locally and verified
  normal process exit and temporary-profile removal. No real login or website was
  opened and no mail was sent.

The first actual-browser attempt rejected macOS's normal admin-group-writable app
executable. The launcher now explicitly trusts the chosen local program and local
OS administrators while refusing a world-writable or foreign-owned executable;
private state/profile ACL and mode checks remain unchanged. Later sandboxed runs
were blocked by macOS Chromium startup IPC/Crashpad permissions. The same bounded
blank-window test passed outside that development sandbox. Startup stderr remains
suppressed in the product; no diagnostic page data or browser endpoint is exported.

This is device-session groundwork only. Actual site policy/test-account selection,
safe domain-specific observation, password/TOTP use and original-mission integration
still require the separate site adapter. Linux compilation and independent review
of the integrated change are recorded with the cumulative capability acceptance;
this record does not claim Linux GUI or real-provider acceptance.


Independent review found a local lock ordering regression: a journal failure could
return before clearing the unlocked vault. Integrated `lock_local` now drops it
first and an actual journal-size failure test proves retirement. Re-review closed
the finding. Linux lifecycle3 + CLI lock failure1 passed; process-group0/1 sentinels
remain conservatively unknown during retained cleanup. No site adapter was added.
