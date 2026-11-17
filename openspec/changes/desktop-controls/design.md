# Design

The existing signed `moosuri` executable gains a `desktop` entry point. macOS uses
tray-icon0.24.2 and the AppKit main-thread event pump; Linux uses ksni0.3.7's pure
Rust StatusNotifier/D-Bus backend. No Electron, webview, GTK or system C dependency
is added. Windows WSL2 remains a Linux service plus web shortcut.

One local worker serializes lifecycle commands and probes readiness without model
calls or account access. Shared setup/service operations enforce managed-file
identity. Manual start and login-autostart are independent. A separate desktop lock
prevents duplicate menus without holding the server's update admission lock. A
short-lived control lock serializes start/stop/autostart/removal commands.

The installer offers desktop integration only in a desktop session. macOS receives
an owned application shortcut and separate login LaunchAgent for the menu. Linux
receives application/autostart Desktop Entries. Generated paths/content are tracked;
changed or unrelated entries are preserved. Interrupted installation resumes exact
missing entries. Uninstall signals its durable removing state, waits for the menu
to leave, then removes only managed runtime/desktop files and retains private data.

The tray displays operational state, start/stop, login-autostart, chat/admin links,
concise diagnostics and quit-with-server-retained. Normal web authentication still
applies. Linux shells need a StatusNotifier host (GNOME may need its AppIndicator
extension). GUI availability is distinct from server readiness. System/user policy
may prevent auto-start; no sudo, host startup policy or WSL configuration is changed.
