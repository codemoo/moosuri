# Capacity is one responsibility; authority stays with each transition

`missions/capacity.rs` owns three bounded questions: whether a new mission fits,
whether an existing mission can occupy an owner waiting slot, and whether an owner
has a runnable slot excluding the current mission. New work counts all outstanding
missions against 16; each tenant/account has one runnable slot and two waiting
slots. Paused work occupies the runnable slot.

Existing wait and resume transitions do not consume another global place. They
keep their original transaction, admission-order queue, authority/deadline checks,
completed receipts and lease retirement. Native foreground chat does not acquire
mission limits. Native continuation and stable mission requeue keep their different
guards; sharing their superficially similar attempt SQL is outside this change.

Verification combines existing mixed-wait, paused-owner, guidance and device
continuation scenarios with one public-Store flow at global capacity. Sixteen real
synthetic owners/tasks reject new work while the first task can still wait, accept
an answer, replay that answer, pause and resume with unchanged accounting/profile.
Cancelling another task frees a place for a formerly blocked request.
