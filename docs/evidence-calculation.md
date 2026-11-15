# Exact calculation bound to a current saved source

`source_calculate` searches one current saved source, reads it and performs exact
arithmetic in one typed call. `evidence_calculate` retains the compatible explicit
search/read-receipt route. It does not choose a source among multiple matches or extract numbers
by meaning. A current request supplies a literal search phrase, an exact numeric
field prefix and all other decimal inputs. For example:

> 🌱 자료 Garden order C; 숫자 앞 문구 unit_price:; 수량 3; 수수료 0.25.
> 자료의 숫자에 수량을 곱하고 수수료를 더해줘.

The model sends those exact query/prefix/scalar **quotes**, never UTF-8 offsets or
its own extracted source amount. Query and prefix must occur literally in the
current task. Repeating the identical selector in a report title is allowed: the
server records its first exact occurrence. This never relaxes uniqueness of the
actual source search result or the field inside that source.
Each numeric quote must resolve to exactly one complete numeric token in the current
user message; a digit inside another amount is not another token. Thus quantity `5`
and fee `0.65` can coexist, while two separate quantities `5` still require clarification.
The server resolves disjoint UTF-8 ranges itself. A missing, repeated complete numeric token, overlapping
range or attached numeric substring is rejected. Korean
and emoji before a parameter do not change this contract. Values only available
in history, another message or a learned example cannot fill a current parameter. A single sentence-final period may delimit a current numeric
quote. Only explicit ordinary separators (whitespace, quotes, brackets and
semicolon, with bounded comma/colon/terminal-period handling) delimit numbers.
Grouping and expression fragments such as `1` in `1,234` or `3` in `1/3`,
including adjacent Unicode operator symbols, are rejected. Common Korean particles, counters and copula endings can attach to a complete current number, as in `4.08을`, `4개를`, `4상자씩` or `2.64야`. The entire supported suffix must end at an ordinary separator; partial numbers, arbitrary words and adjacent expressions remain rejected. Bare scaled numbers such as `4만` remain invalid; `4개만` has an explicit counter and is accepted. This parses the literal amount and does not infer or convert units.

## One unambiguous complete source

Both paths execute `knowledge_search(kind=source)` with the current
literal phrase, no filters and no cursor. Exactly one hit, no next cursor and no
truncation are required. The source read must use that exact current identity and
revision, `start_byte=0,max_bytes=4096`, and return a complete nonempty source.
Ambiguous search results require a narrower current-user phrase/title; the first
of several matches is never sufficient. A bounded incomplete source requires a
smaller complete source, not guessed coverage or hidden continuation.

The explicit route references actual earlier same-run search/read receipts. The
fresh route creates child search/read receipts0/1 in the same transaction as its
result; arithmetic receipts follow them. The model cannot supply child addresses
or substitute a remembered source amount. It rechecks current search uniqueness and identity/revision,
compares the complete current document with the read result, and requires exactly
one occurrence of the user's fresh prefix. After only ASCII space/tab following
that prefix, it parses one whitespace-delimited plain signed decimal. The server
derives the literal value and its actual source byte range. Exponents, currency
symbols, thousands separators, expressions and missing/ambiguous fields are not
converted. The existing six-fractional-place, size and checked arithmetic bounds
remain. Source text is untrusted data, never execution authority.

## Explicit structure and actual receipts

A plan contains input origin kinds (`source` or `current`) and a typed1–4-step
sum/difference/product graph. Exactly one input is the source number; the other
inputs are fresh current-message scalars in declared order. Every input and step
must contribute to the last result. Graph constants, self/forward references,
unrelated extra operations and equality-inferred edges are rejected. The existing
2–4-step `decimal_workflow` contract is unchanged; the bound arithmetic primitive
also supports a single operation through the same checked evaluator.

Actual arithmetic operations have private child input/result receipts linked to
the parent slot. The typed result records the selected source identity/revision,
title, literal numeric range, actual read addresses and server-resolved current
parameter ranges. It does not prove that a literal field is semantically suitable
for the user's wider decision, that units match or that the source assertion is
true. Missing semantic/quantity/unit information still requires clarification.

## Learning and fresh reuse

Two distinct finalized current-owner runs must use either explicit bound tool, use
distinct current source subjects and canonical parameter inputs, and adopt the
actual source title/prefix/number and exact final result in the terminal final
answer. Earlier commentary, unused tools, a success flag or a model claim do not
qualify. The Store re-resolves origin bindings and independently recomputes every
arithmetic child receipt. Separate reads and ordinary calculations are never
silently inferred into this mixed plan.

The private route stores only origin-slot kinds and canonical arithmetic
structure. It stores no source IDs, search phrases, field prefixes, values,
byte offsets or training text. Existing maximum64 routes/tombstones, candidate/
usable states, owner isolation, hide/disable/delete and withdrawal contracts apply.

A fresh learned invocation accepts only current-user query/prefix/scalar quotes.
Within the existing learned-route executor it reuses the normal source primitive
with strict uniqueness, performs new search/read calls and evaluates the pinned
structure. Read children0/1 and arithmetic children2 onward retain their actual
inputs/results and costs. This is not a new orchestration runner. Source edits,
deletion, disconnection, proof-thread withdrawal and route disable/remove retain
the existing dependency and late-output fences. Discovery alone is not use.

The fresh tool reserves two reads plus its1–4 arithmetic operations and charges
actual work, including failed reads/calculations. It reports bounded corrective
errors for invalid current parameters, nonunique source selection and invalid
source fields. Missing sources remain `not_found`.
Direct receipt-bound arithmetic reserves/charges its actual1–4 operations; prior reads
were already charged. Learned execution conservatively needs6 remaining tool
units and charges the actual search/read/arithmetic work. Missing search results
charge one read; a rejected binding after two reads charges those reads plus the
minimum failed-tool unit. Same-slot replay returns the committed result without
new reads, child receipts or tool charge. Existing6model/12tool/120second bounds
remain. Missing/ambiguous current evidence or unsupported current parameters do
not count toward three-attributable-failures quarantine; arithmetic overflow does.
Fresh source calculation is available to foreground chat and accepted one-off or
recurring missions. Mission parameters bind to the current accepted task and
constraints through the same authority and provenance checks. A proven pinned local recipe can be reused in an explicitly created one-off or recurring mission. Its literal parameters bind to the current accepted task/constraints, never serialized request IDs, verification answers or original/replaced history. Each execution re-reads current evidence and retains the existing child-call budget and withdrawal rules.

## Generation and sharing

Native toolset generation3 replaces generation2 eligibility. The new additive
`evidence_calculation` profile flag defaults false when absent in older persisted
profiles, and only fresh foreground profiles gain it. Old runs retain their old
vocabulary and cannot qualify as generation3 examples. Original private runs,
observations and tool receipts remain; the existing schema-drift policy retires
the disposable eligibility proof index. Two fresh independent examples are needed
before an invalidated procedure becomes usable. The additive `source_calculation`
flag also defaults false for old persisted runs. Fresh profiles enable it without
changing existing generation3 recipe structures or retiring their valid proofs. Tombstones, user disable and
failure quarantine are not cleared by schema drift.

Explicit sharing reconstructs the four existing instruction-package files from
the plan only. It generates a repository-owned synthetic source precondition,
query/prefix and input values, and calculates the synthetic expected result. The
recipient must supply fresh parameters and actual current source evidence. An
absent synthetic fixture returns missing evidence; its example value is never a
fallback. Installation does not create a source, perform a lookup, transfer a proof
or grant authority. No private anchor, source ID, amount, account, run link or trace
is an export input. The existing package checker and license contract apply.

Public URL adapters, multiple source joins, JSON/CSS selection, semantic field
extraction, transformations/unit conversion, multimodal inputs, arbitrary code and
external writes remain unsupported. Synthetic evidence is recorded in the
[dated review](reviews/2026-11-15-evidence-calculation.md); it is not a real-model
quality or full F26/P4 acceptance claim.
