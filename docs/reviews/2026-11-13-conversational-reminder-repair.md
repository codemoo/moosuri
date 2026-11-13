# Conversational reminder repair: SC1–SC3

The original26-file implementation received changes requested in independent
review. This is a separate repair copy; it preserves that frozen source and does
not claim fresh independent acceptance or combined schema22/media integration.
No public API, schema, weekly recurrence, frontend or provider/tool behavior was
added by this repair. Tests use only synthetic accounts, file-backed SQLite and
offline model configurations. No live effects, commits, push or CI were used.

## Repairs

- **SC1:** Parse a complete control imperative and explicit target selector before
  mutation. Questions, conditions, deferred actions, alternatives and quoted
  controls do not grant authority even with a selected receipt. Resolve only a
  generic unique/selected target or exact owned parsed title, never an arbitrary
  message substring. Explicit titles must also match a selected receipt. Literal
  leading `the` is retained in named titles; it cannot select a different `water`
  reminder when the requested title is `the water`.
- **SC2:** Check known task verbs on parsed Korean nonliteral action content for
  one-off and elapsed requests as well as recurring requests. `내일 오전 9시에 뉴스
  조사해서 알려줘` now returns the fixed unsupported-mission result. Explicit
  literal framing such as `뉴스 조사하라고 알려줘` or an 알림 marker still saves
  the fixed message the user requested.
- **SC3:** Separate explicit reminder/change-message shells from literal payloads.
  `remind me in 10 minutes to summarize my notes` stores the exact message and
  completes immediately with a schedule receipt; no model run is claimable.
  Translation/quote/negation words and punctuation inside the payload are data.
  Outer `Translate: remind me ...` remains ordinary translation input.

The repair remains deliberately bounded. Supported clear commands act immediately
without another approval; unsupported control syntax clarifies. No semantic
classifier, broad natural-language expansion or stateful follow-up was added.
Conservative control-shell syntax may require selecting a receipt and using a
generic imperative when a title itself contains punctuation/conditional words.

## Outcome tests

`repair_controls_require_a_complete_current_imperative_and_exact_owned_title`
uses actual Store admission with/without selected revisions. Original reviewer
examples and additional conditional/question/alternative/quoted controls leave
one active revision1 schedule, no command receipt and no model claim. Wrong full
titles cannot silently redirect selection. Clear exact-title pause/edit/delete and
generic selected resume still act directly; a two-title collision preserves the
untargeted reminder. Message edits retain literal meta/negation/quote content.

`repair_one_off_missions_are_declined_and_literal_meta_words_still_save_reminders`
checks one-off, elapsed and recurring unsupported tasks, exact saved bodies,
zero model claims for reminders, a literal Korean message while a different receipt
is selected, and the outer translation path. Existing ambiguity, transaction,
replay/reopen, ownership/revision, epoch, pending-delivery and DST tests remain.

Own repair counts and exact source/log hashes are recorded in the private frozen
repair delivery. Inherited original214 Store/188 React counts are not fresh repair
verification. Independent reassessment and root schema22 integration remain next.
