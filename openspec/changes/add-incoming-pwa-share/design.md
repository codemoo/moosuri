# Design

Use bounded stream read (32KiB,8s) then the browser multipart parser. Receive only
same-origin exact navigation POST; reject duplicate/unknown/file parts and more than
8192 combined UTF-8 bytes. Treat every value as inert text.

Separate IDB from Push and Cache API: one payload,5min absolute expiry, random ID,
device/generation/intended owner/original scope, atomic single-window claim. Fresh
session GET is mandatory before claim and insertion. Ordinary401 retains a known
owner opaquely; wrong account rotates/erases, and unbound401 discards. Session GET
failure reveals nothing. Preserve the last authenticated origin scope during401,
including a share staged after expiry. Before disclosure/insertion, owner-authenticated
POST /api/v1/shares/origin carries both origin_scope and expected_current_scope.
The latter must exactly match the actual authenticated cookie UserContext namespace
under the delivery guard, including all final disclosure and insertion proofs. Thus
a cookie replaced between GET and POST cannot migrate an existing claim, while a
fresh B claim of an unclaimed A origin remains valid. The original namespace proves the exact original namespace is a retained current-
epoch unrestricted session row. The Store scans at most257 owner rows, fails closed
above256, and never renews the origin. Natural expiry remains provable; individual
logout/account revoke/password/suspend/recovery epoch/deletion rejects it. No admin
bypass, new bearer, migration or dependency is introduced.
Explicit logout/password/cross-tab transition synchronously fences memory and rotates
durable generation. Postcommit completion is not disclosure authority: fresh auth/
origin checks precede a final durable IDB transaction whose synchronous callback is
the only review/draft write. Independent contexts cannot rely on a shared memory epoch.
Retirement hints hide displayed review immediately; no hint is required to fence late
claim/consume delivery. A retired exact scope blocks old-cookie intake while logout is
pending. Every delayed write compares captured generation. BroadcastChannel is a hint.

An explicit new OS share replaces an old payload/claim. Payload-free tombstones stay
until original TTL, bounded to32. No claim transfer: reload/page loss requires re-share.
Consume erases before draft append; a crash may lose the handoff, never duplicate it.
Expired payload is purged on access; a closed browser cannot promise timed disk erasure.

Use real Vite module bundling for the worker, preserving generic Push and no
skipWaiting. URLs carry only opaque fragment IDs. No audio/server/schema/dependency
change. Append targets the original slot and checks the latest draft's UTF-8 bound;
normal Send remains the sole chat mutation.
