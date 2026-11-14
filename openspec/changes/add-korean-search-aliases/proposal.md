# Add opt-in fixed Korean search aliases

## Why
F22/P3 names Korean synonym retrieval. Existing literal normalized FTS/substring search cannot find a different listed public spelling without a second caller-supplied query.

## What changes
Add optional versioned literal/Korean-alias text matching, limited to three immutable public families and at most three complete query variants. Support authenticated API, native read tool and existing React filters. Preserve old default serialization/results and scope/lifetime filters. Decline unsupported alias learning without changing generation3.

## Impact
Core search DTO, small public dictionary, Store matching and learned eligibility, native schema/parser, React selector, fixed hand-authored gold and focused lifetime/pagination tests. No migration, dependencies or provider calls.
