# Design

The selector first chooses the newest begun memory revision, then filters its
expiry inside SQL before the65-row completeness probe. The64-current-row and
32KiB payload bounds remain. Older versions cannot reappear after expiry. Full
snapshot/generation and current RunLease validation remain at selection/commit.

Store chat context formats original `created_at` plus `original_timezone`, using
chrono-tz for historical DST offsets. No timezone means explicit UTC fallback;
invalid stored timezone/timestamp fails closed. Retry retains those stored fields.
One trusted System message contains the reference; runner projection merges trusted
system messages into the initial system instruction. Tool/media instruction
replacement reapplies the same trusted metadata. History trimming therefore
cannot remove it and Anthropic never receives a late system turn. The isolated
memory classifier remains current-user-only and receives no added write authority.

The read-only shortcut recognizes complete bounded Korean suffixes with an owned
noun-target case or explicit “얼마나 되는지” form. Nested targets and conservative
clause/predicate markers retain classification independently of the verb cue list. It does not interpret arbitrary natural
language. Quotes, compound commands and unmatched forms retain classification.
The ordinary tools retain their existing current-owner, receipt, revision and
budget checks.
