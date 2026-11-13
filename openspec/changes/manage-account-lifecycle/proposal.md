# Account administration with durable revocation

Administrators can create users but cannot stop access, change roles or recover an
expired invitation through the current UI. Add suspension/resumption, role changes,
24-hour temporary-password reissue and all-device sign-out to the existing people
screen. Each action retains private data and adopts a new authentication epoch;
resumption never rewrites an old session or background grant to the new epoch.

Do not introduce repeated approval dialogs. Mail delivery, account erasure, vault
account outbox and provider budget administration remain separate changes. This
unit does not implement or claim those operations.
