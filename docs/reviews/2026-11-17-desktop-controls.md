# Desktop server controls acceptance

The desktop menu uses the existing signed Rust binary and shared setup/service
operations. Chat and all management remain in the existing authenticated React web
app; no local HTTP control API, shared account session or credential reader was
added. The menu's server stop applies to every member and scheduled task. Menu
process exit leaves the server running.

## Implementation and review

macOS uses tray-icon0.24.2 with the AppKit main-thread event pump. Linux uses
ksni0.3.7 with pure Rust D-Bus/StatusNotifier support, without GTK/libappindicator
runtime libraries. Both remain within Rust1.89. Desktop shortcuts and login entries
are tracked as exact owner-managed files; headless setup is unchanged and WSL2 uses
a web shortcut. The menu uses the existing pixel attendant artwork.

A frozen independent review identified four P2 issues: an already-ready legacy
installation skipped launcher refresh, browser dispatch could block the worker,
stdout inherited by a manager's descendant could defeat its deadline, and large
status-only output could incorrectly make launchd unavailable. All were fixed;
the second frozen review found no remaining material findings in that delta.
Regression contracts cover deadline/pipe behavior, large status output and browser
dispatch. The final later change only reuses/decodes the existing product artwork,
with a targeted decode check and refreshed platform builds.

## Evidence and limits

- Native CLI installation contracts20/20 and two real-binary English/Korean PTY
  flows passed, as did scoped all-target Clippy. Linux contracts19/19 and the same
  two PTY flows passed with scoped Clippy. Counts differ because the ACL contract
  is platform-specific. One initial Linux test fixture needed explicit0700 mode;
  the real installer already created owner-private directories.
- On a macOS ARM64 graphical host, setup created a **new isolated synthetic**
  launchd installation after checking that its service label was absent. Actual
  readiness204, stop/start, and login-autostart off/on all passed. Disabling
  autostart kept the server running; manual start preserved the disabled override.
- With a ready server at a custom prefix, a synthetic legacy launcher was refreshed
  on setup rerun. New desktop/autostart commands retained the same custom prefix.
- An actual menu process stayed alive; duplicate launch returned without another
  process. Terminating only that process left server readiness intact. A subsequent
  interactive uninstall caused a fresh menu to exit through the removing state,
  removed the synthetic launchd service/shortcuts and preserved every data-file hash.
  No prior operating service, collector, cron, account or private DB was used.
- Computer Use could not inspect the accessory app and timed out. Native menu-click
  and visual acceptance are **not claimed**. macOS lifecycle checks exercised the
  same control operations used by menu actions; they do not replace that UI check.
- Four release targets build: macOS ARM64/Intel and Linux x64/ARM64. Linux x64 release
  runtime passed isolated init/app/login/shutdown/doctor; Linux ARM64 passed the
  same checks under QEMU. Intel was cross-built. Actual Linux panel/systemd session,
  WSL2 and physical ARM device acceptance remain open. GNOME may need a StatusNotifier
  extension; missing panel support is separate from server health.

This is a development preview, not stable-release or previous-service deployment
acceptance. Ed25519 package verification is separate from Apple signing/notarization.
No live provider call or CI activation was used. Public package identity and bootstrap
acceptance follow in the release publication record.
