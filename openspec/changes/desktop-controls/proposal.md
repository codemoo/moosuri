# Desktop server controls

A completed installation should remain usable without remembering terminal commands.
Provide a small Rust menu-bar/tray surface for F01. Keep chat, account/model settings,
connections and usage in the existing authenticated React web app.

The desktop controls the installed server, including all members and scheduled work.
It is not a per-user pause switch. Closing the menu leaves the server running.
Headless installations retain CLI/user services. WSL2 receives a web shortcut rather
than a native Windows tray. No new provider, credential or local HTTP control API.
