# Read bounded Gmail attachments (F34)

Large PDF attachments currently exceed the raw-message cap and prevent even body
reads. Use Gmail's official full-message MIME tree and per-message attachment API
so the authenticated user and admitted read-only model tools can inspect exact
attachment metadata and bounded PDF/plain-text pages. Reuse existing revocable
mail receipts and the deployment-owned PDF child. Sending attachments remains
unsupported; no database migration or new third-party dependency is required.
