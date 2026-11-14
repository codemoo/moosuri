# Primary Calendar all-day solo events

Bounded implementation based on64d0a16. Existing Calendar fields already retain date-only reads, so no DB migration or OAuth scope change is needed. Direct forms and current-owner chat commands use the same durable CalendarService/Store effect lifecycle and receipts.

All-day API fields are strict date-only start/exclusive-end pairs with null timezone, bounded to31 civil days. Connector requests use date objects, never timestamp values. Observations reject mixed/dual date/time shapes and retain organizer/primary/solo/nonrecurring/notification checks. Store and connector independently reject timed/all-day conversion. Reconciliation requires exact civil dates, kind, operation/digest marker and current eligibility. Unknown results remain lookup-only and retain the original action UUID.

Forms expose inclusive 시작일/마지막 날. Chat interpretation adds nullable exact all_day_quote and inclusive last_date/last_date_quote values; older saved proposals deserialize with null values. Store checks exact grounded date quotes using the frozen reference/IANA zone and adds one civil date for the API end. A date move retains the observed civil-day count, including across DST. Existing bounded continuation freezes these fields and accepts only missing values from the actual current answer. Exact immediately preceding applied receipts supply identity only; fresh ETag and current owner intent still control mutation. General history is not new authority.

Passed synthetic checks:

- Native all_day7: counted Store/service form effects, same-thread create→DST move→delete with exact target, missing-last-day continuation, malformed ranges/shape, date oracle and conversion fences, actual loopback provider date wire/If-Match and lookup shape.
- Affected timed3: Store seconds/duration/DST move regression; actual connector timed write/ETag/delete204; sealed service timed update/delete.
- Calendar React15, TypeScript checks and production build.
- Generated API types exported from source and exact comparison passed.
- Core/connectors/Store/agent all-target Clippy with warnings denied.

Fixtures use synthetic owners/tokens, counted typed IO or loopback HTTP only. Natural-command fixtures supply deterministic typed interpretations; they do not establish broad language understanding or model accuracy. No real Google/ChatGPT connection, credential access, provider write, deployment or production readiness is claimed. Root owns independent review and integrated Linux acceptance.

## Independent and integrated acceptance

Root verified all20 frozen source hashes and independently traced strict civil-date
validation, current-owner intent/quote checks, receipt-linked current ETag admission,
kind-conversion rejection, provider wire/projection, exact unknown reconciliation
and inclusive UI dates. Existing membership, notification and external effect
boundaries remain in force. No material finding remained. Linux passed all_day7,
the selected actual timed connector lifecycle, affected all-target Clippy and the
default assistant build. Integrated React typecheck/build also passed. The final
Linux binary build with the integrated React assets also passed; earlier native
checks were not rerun just to increase counts.
