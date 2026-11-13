# Add opt-in generic Web Push

## Why
The durable reminder inbox needs optional device delivery without exposing private
reminder content or confusing browser permission with account authentication.

## What Changes
- Add private generation-bound enrollment, a bounded fair queue and honest service receipts.
- Use maintained Web Push encryption and fixed ES256 VAPID signing through a strict HTTPS/DNS policy.
- Add explicit React enrollment/settings and service-worker generic display/click handling.
- Fence current auth/session/epoch, read/dismiss, quiet hours, account switch and stale acknowledgements.
- Add runtime-key inventory validation plus staged export/restore retirement hooks.

## Impact
Assistant migration0023 reserved (isolated schema19 after baseline18), core/store/
connectors/agent/server and React/PWA. No live subscription/provider send, new model
call, mail approval bypass, vault/media/recovery or schedule-chat implementation.
Root owns final recovery integration, independent review, Linux/device tests and publication.
