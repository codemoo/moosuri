# Design

The vault lists its own Google login items, exposing only title and email in the
selection UI. Zero items permits email/password entry and encrypted save; one
item is reused; multiple items require a selection. The member email remains
the recipient enrollment identity. The selected email is an additional RSA-OAEP
and AES-GCM authenticated field, and cannot change on a Deliver replay.

Schema 103 adds an optional selected email to the existing OAuth account-choice
record. Existing rows continue to require the member's email. The callback
checks Google's verified mailbox against the stored selection before committing
credentials. Each request keeps its original member, session, epoch and service.

Missing installation configuration does not hide the vault entry point. It
remains a distinct incomplete outcome, with no OAuth success or API-read claim.
The official client-creation boundary is documented at
https://developers.google.com/identity/protocols/oauth2/resources/best-practices#manual-client-creation.
