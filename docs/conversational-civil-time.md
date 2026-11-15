# Conversational civil time

Reminders, delegated read missions and Calendar share deterministic civil date and
clock handling. The Store checks supported literal source expressions against the
captured request instant and actual IANA timezone. Models may identify literal
fields; their computed dates, recurrence and clocks do not become authority.

Supported day forms are today/tomorrow/day after tomorrow and 오늘/내일/모레/글피.
A bare weekday means its upcoming local date, including today. `this Friday` /
`이번 주 금요일` means Friday of the current ISO week. `next week Friday` /
`다음 주 금요일` means Friday of the next ISO week. A same-day past clock is
rejected rather than moved to the following week. `next Friday` / `다음 금요일`,
a week without a weekday, weekend, morning alone, missing clocks and conflicting
clock markers require clarification rather than an inferred instant.

Explicit clocks include numeric 24-hour forms and AM/PM, Korean native hour words,
Sino-Korean minute words and 반. A malformed complete clock such as 오전9시60분,
오전13시 or 09:60 cannot fall back to an earlier valid prefix. Bare 1–12 hour
clocks still require AM/PM. One-shot local times that fall in an IANA DST gap or
fold are rejected; the established recurring scheduler DST policy is unchanged.

Mission and Calendar acknowledgement compare each temporal quote with its complete
overlapping source span. Removing `next week` or an explicit recurrence marker
from a quote cannot turn the request into a different day or one-shot command.
Explicitly quoted task/title data cannot supply temporal authority. Unrelated
nonoverlapping task/title words also do not revoke a valid scheduling expression.
Weekly missions require explicit recurrence language; a bare weekday cannot become
a weekly mission and an explicit weekly request cannot become a one-shot mission.

Existing reminder drafts and mission/Calendar continuations retain their original
supplied temporal components, reference instant, timezone, owner/session and
bounded continuation budget. A missing-time answer follows the existing answer
anchor contract; an already supplied day or clock is not recomputed at reopen.
Literal fixed reminder content remains inert. `내일 날씨 알려줘` remains ordinary
chat. Cheap routing cues do not add an ordinary-chat model classification call.

No new schema, dependency, public API or timezone preference storage is introduced.
These are bounded Korean/English forms, not arbitrary natural-language temporal
understanding. See the [review record](reviews/2026-11-15-conversational-civil-time.md)
for authored outcomes and the distinction between source checks and integration.
