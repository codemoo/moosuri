# Compose current-request planners through typed dispatch receipts

Ordinary saved-document requests may enter the artifact classifier and receive NoCommand before mission routing. The mission planner currently mistakes that run-wide remote-call count for its own attempted dispatch and rejects the request as authority revoked. Explicit task and Calendar requests after that harmless stage also fail the first-call assumptions.

Add durable domain/step/attempt/nonce/lease provenance, admit the mission classifier atomically with its dispatch receipt, and permit only an exact completed no-command artifact predecessor. Preserve the artifact-first write exclusion and all authority, completion, media, tool, revocation and shared-budget fences. No older ambiguous history receives a new dispatch capability.
