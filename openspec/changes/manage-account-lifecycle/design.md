# Design

Use accounts.status and management_revision, plus content-free request receipts.
The Store prepares reset-password hashing outside the delivery gate, then consumes
an opaque preparation after rechecking the current administrator and target CAS.
The HTTP commit holds the existing delivery write fence. Adoption appends/fsyncs
the opaque MSAUTH01 epoch before the atomic role/status/password, session removal,
receipt and journal watermark transaction. A failed post-append transaction poisons
the actor; reopening locks the affected account for explicit local recovery.
Validated auth-journal opening also fsyncs before returning replay authority.

User password changes and local operator recovery increment management_revision.
Suspension blocks login and authentication; every change increments auth_epoch so
old background grants and in-flight authority cannot resume after reactivation.
Administrator metadata contains names/emails, role, state and revision only.

Request UUID and exact digest make lost-ACK retries return the same receipt. A new
temporary password is returned once, never stored in the receipt or browser
persistence. An acknowledged retry returns no password and explains how to issue
another explicitly. Role/status conflicts never silently retry against a new head.
The caller cannot suspend, demote or reset its own account through administration;
its ordinary password settings and all-device sign-out remain usable. Other fully
active administrators are required before retiring another active admin's access.

The existing React people list carries managed metadata with each new invitation.
Account request lifetimes, per-refresh cancellation and synchronous operation
ownership fence late responses and duplicate clicks. A returned password is shown
only while the same account/target revision still owns the response. No new
credential enters localStorage, IndexedDB, service-worker caches, diagnostics or
Git. Clipboard transfer remains explicit and browser-owned.
