# Design

The public seven-key classification decision stays unchanged. A pure shared parser
recognizes only a leading `DATE부터 [DATE까지], FACT` or `DATE까지, FACT` directive;
newline may replace the comma. DATE is exact `YYYY-MM-DD` or Korean `YYYY년 M월 D일`.
A Gregorian date must be valid and a range ordered. The entire current message is
scanned for recognized additional effective expressions, even if the model omitted the date
from its literal fact quote. Plain birthdates and event dates remain facts.

Admission captures original ScheduleChatContext.timezone in the existing stored
model-choice JSON, independent of mutable continuation/calendar profiles. Store
converts date-only bounds with chrono-tz: start at local midnight inclusive, end at
next local midnight exclusive, including the requested end date. Missing timezone,
ambiguous/nonexistent midnight or values outside existing memory bounds clarify.
No host clock, relative-date inference or model timestamp is accepted.

Existing plan JSON holds a versioned internal envelope containing the unchanged
intent and server scope. Loads compare the sealed scope with derivation from the
immutable run text/timezone. Legacy undated plans recover; malformed seals and
legacy temporal plans fail closed. Pending target plans use the same envelope;
target selection, generation checks and head/effective CAS are unchanged.

Temporal clarification happens before target selection and uses no additional model
call. Successful confirmation reads the actual saved revision's bounds and shows
inclusive start/end dates in the original timezone, reconstructed from actual
saved bounds (end uses the local date at the saved exclusive end minus one second). The terminal receipt
shape remains content-free. Dated forget requests clarify instead of scheduling a
future deletion. Existing journal, invalidation and run-wide budget paths remain.

Recognition is deliberately bounded Korean/English syntax, not general temporal
language understanding or a semantic classifier accuracy claim. Ambiguous vocabulary
such as 기준/적용/through/until requires explicit nearby date/relative-time context
or an explicit effective-boundary label. Bare words in ordinary facts do not block
undated writes, dated fact bodies or legacy prepared undated recovery.
