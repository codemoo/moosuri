# Personal Gmail and explicitly approved sends

Users need the assistant to read their own inbox and prepare replies. A request to
send mail must produce an exact preview and wait for explicit final approval.

Add per-user OAuth, bounded read tools, local immutable drafts, a human-only approval
endpoint, a durable send worker and a React connection/preview interface. Keep these
credentials separate from shared model providers and from the future user-key vault.
No live outgoing mail is authorized by development or synthetic test instructions.
