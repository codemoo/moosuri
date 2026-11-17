## Implement
- [x] Add Rust macOS/Linux desktop menu using the existing signed binary and web routes.
- [x] Separate server start/stop, login-autostart and menu lifetime; bound service-manager calls.
- [x] Add owned application/login shortcuts and preserve headless/WSL2 behavior.

## Verify and publish
- [x] Verify readiness/error states, autostart independence, menu lifetime, removal and ownership boundaries.
- [x] Complete independent review and macOS/Linux build/runtime checks with actual-platform limits recorded.
- [ ] Publish updated signed installer packages, instructions and acceptance; keep CI disabled.
