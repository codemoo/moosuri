# Follow an actual Gmail conversation without polling a model

Users can ask the assistant to wait for a reply in the conversation where they
read or sent the original email. Existing task monitors check saved-source changes;
they do not query a remote mailbox. F34 needs a bounded provider check that
consumes no model tokens when nothing changes and never claims unregistered work.

Add an exact-thread, next-reply watch from owned completed foreground read or
confirmed send receipts. A metadata probe precedes any new body read. Show literal
verified excerpts in the original chat and notify through the existing inbox and
opt-in Push. Preserve mandatory final-preview approval for every outgoing mail.

This slice uses server-managed Gmail OAuth. Automatic semantic summaries,
mailbox-wide/event interpretation, native-device background Gmail and telephone
calls remain outside this change. No personal source collection is created or
broadened. No real provider account or outgoing mail is used for acceptance.
