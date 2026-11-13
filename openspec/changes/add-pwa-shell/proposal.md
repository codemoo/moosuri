# Why

F16 requires an installable React app and offline shell without storing private
messages, credentials or API responses in the service worker.

# What Changes

- Add production-only service worker registration, a versioned public shell and manifest.
- Add browser-provided install action, offline status and boot reconnection.
- Use a new original pixel attendant icon for installed apps.
- Preserve drafts in mounted tabs; updates wait for old tabs to close.

# Impact

React/TypeScript build, public assets, embedded static-server acceptance. No private
offline queue, share-target intake or push subscription is included in this slice.
