## Why
A held first image upload blocks all later selected images inside the serialized decoder queue. Separately, an older library response can replace an already-ready upload with retry_required and remove its Attach control.

## What Changes
Keep image decoding/resizing serialized, but let each prepared image upload independently. Give library refreshes an immutable controller owner so newer refreshes cancel and supersede earlier responses/errors.

## Impact
Frontend Media.tsx and focused synthetic UI observations only. Existing four-attachment, immutable ID/bytes, cancellation, deleted-media and account-lifetime boundaries remain. No API/store/provider/model/effort or voice behavior changes.
