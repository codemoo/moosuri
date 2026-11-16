# Design

Current explicit reminder commands execute in the existing Store transaction.
The parser produces an offset only; target resolution supplies the owned saved
one-off timestamp. Previous/that targets retain immediate same-thread receipt,
session, epoch, recency and revision checks. Exact titles and generic sole-owned
controls retain their existing semantics. Request replay returns the saved run
before any offset can apply twice.

Only positive, fully consumed duration components in descending units are
accepted, bounded to366days. Postpone/advance changes a one-off by elapsed seconds.
Past/out-of-validity times clarify without mutation. Recurring reminders require
an exact replacement clock/rule because changing one occurrence versus the whole
series is ambiguous. There is no conversion of recurring reminders into one-off
schedules. Other reminders and identity/revision history remain unchanged.

Unsupported semantic task decisions use a shared finite reason vocabulary in
schema and Store. The model does not write the user-facing confirmation and no
extra model round is added. Existing malformed decision validation stays in place.
