# Complete Google vault selection

Google connection must examine the member's Google logins before starting Kiri,
including when installation OAuth configuration is missing. The existing flow
only searches the Moosuri login email, so it cannot select a different Google
account owned by the same member.

Keep member authority separate from the selected Google email. Bind the selection
to the sealed Kiri handoff and the pending OAuth request; reject a different
provider account before storing a grant. No password, code or client secret is
sent to a model or recorded in logs.

Google's documented manual client-registration requirement remains an external
installation prerequisite. Do not claim automatic client creation or completed
Google API access until those actions have actually succeeded.
