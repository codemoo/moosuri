# Design

A small shell bootstrap detects the native target, downloads an immutable GitHub
release archive over HTTPS and verifies the archive checksum pinned in the script
before executing its Rust binary. The script is the initial HTTPS trust root;
its checksum is not an independent out-of-band proof. The Rust installer then
verifies the manifest signature using the public key pinned in that same reviewed
entry point. Existing explicit independent-pin installation remains available.

Rust owns setup, controlling-terminal input, signed runtime activation, ordinary
account bootstrap and authenticated provider APIs. Secrets never enter argv or
ordinary stdout/stderr. English/Korean prompts offer defaults, arrow or numbered
menus and optional model setup. Temporary passwords retain mandatory first change.
Local mode admits HTTP loopback only; HTTPS/Tailscale retain their exact origin.
Neither DNS/proxy configuration nor public Tailscale Funnel is changed.

One installation per OS user uses a private runtime and separate SQLite data.
Recorded exact launcher/service bytes permit resume and removal without overwriting
unrelated files. macOS uses launchd user agents; Linux/WSL2 uses user systemd when
available, otherwise foreground operation. Readiness follows actual startup.
The existing updater remains the only schema migration/activation path.
