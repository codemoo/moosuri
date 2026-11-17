# Design

Use Tailscale v1.104.7 LocalAPI: native Unix transport on Linux/open-source macOS;
protected sameuserproof-authenticated loopback on GUI macOS. Restrict requests to
fixed local transport, discard raw replies, and never persist authentication material.
Login uses the existing profile's interactive flow, not CLI login's empty-profile switch.

Read the current node DNS/certificate domain and Serve ETag. Require a dedicated
HTTPS443 root and reject unowned, shared-origin, foreground or Funnel collisions.
Patch a preserved copy using If-Match. Never use serve reset or CLI root replacement.

A private durable receipt joins external CAS with installed origin/examples. It
contains original access mode and exact owned old/new examples. Pending phases
close start/update admission; control/runtime locks serialize host changes. Retry
recognizes a matching pending write without repeating its effect. Disconnect
restores only this installation and conservatively preserves changed external routes.
The current node ID binds ownership across profile switches.

WSL's Windows host daemon is a separate environment. Keep an explicit manual
hostname path; do not install a second daemon or claim automatic named-pipe access.
