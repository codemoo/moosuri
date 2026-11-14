# Native Google INBOX preview review packet

Base993016d. The implementation adds preview-only native selection/policy, a separate typed signed projection, fixed bounded INBOX list/metadata reads, personal settings and original-chat terminal settlement. Existing counts authority and bytes remain separate. No migration/dependency/provider-write/real-account use is introduced.

Source boundary: vault-protocol broker types/encoding; native Google adapter/policy/CLI/live-vault/transport; broker Store registration/receipt validation; current-chat profile/parser/admission/fixed rendering; HTTP/settings. Necessary explicit profile constructors default the new flag false. Public README/status/features are root-owned and unchanged here.

Official Google list/get/Message pages were checked read-only. No order guarantee is claimed. IDs and account checks remain transient/device-local; only requested mail text is projected. Text is bounded with truncation metadata and literal rendering, is not passed to another model, and exact secret/token echoes fail before normalization/truncation. Empty/deleted/moved/unknown outcomes are distinct.

Focused native validation passed:6 Google adapter/journal/signature tests (the final2 preview tests rerun after the last ID-echo guard),6 actual Runtime credential-chat cases,4 Store current-authority/literal/profile cases,3 feature-on broker HTTP cases,1 feature-off absent-route case, and4 personal-settings tests. Counts, GitHub and prior signed variants remain covered in those bounded groups. An independent Python encoder first matches the preexisting GitHub golden hash, then supplies hashes for counts/new preview requests, receipts, projections and registration/refresh commands. Worst-case escaped JSON remains below the signed-command/body bound.

Scoped four-package all-target Clippy with the broker feature passed; Rust formatting/diff checks and web typecheck/build passed. No actual Google credential/account was used. Native Cargo lane is released after the freeze. Generated web dist and the reused node_modules symlink are excluded from the patch.

ChatPanel already renders `message.text` as a React text child, so the fixed original-chat result preserves literal mail punctuation without extra escape characters. Settings also uses React text. Neither path parses mail as HTML/Markdown, follows links or asks a model to continue this read.

The original author snapshot remained based on993016d. Root merged scoped hunks on the published schema51 reminder state, preserving both changes. Synthetic evidence does not certify production Google consent/account/quota, packaging or all P8 adapters.


R1 repairs the independent review findings: durable preview-resource/output markers
exclude the complete preview turn from model history and recall/search after reopen,
without hiding the owner's UI history or relying on current feature flags. Learned
skill proof rejects the display-only source as well. Content/provider errors use a
separate fixed error class and settle unknown; only actual auth/scope/identity policy
rejection suspends Google reuse. New admission reserves8KiB; retained R0 waits use a
bounded honest output-limit notice when needed and always retire the terminal slot.
The new default-false profile flag also omits itself on old-profile reserialization.

R1 targeted validation passed:2 native preview cases now cover bad-content/duplicate/control/malformed/403 outcomes followed by a new successful request under the same grant;1 original count/auth/scope case preserves policy rejection. The actual Runtime preview/restart/cancel case now adds a reopened ordinary follow-up and conversation-search cycle, asserting the mail canary absent from every provider request while owner history retains it. Store's durable-history test passes both feature-on and feature-off, including an old R0 resource record, the newer output marker, a forced retained recall pin and a normal-count positive control. The near-cap Store outcome passes at248KiB, rejects new252KiB admission and settles a reopened retained252KiB wait exactly once with the output-limit notice. The old-profile omission check also passes. These are8 focused test executions; no broad suite/UI repetition was needed because the UI/source protocol bytes did not change in R1.

R1 four-package all-target scoped Clippy, formatting and diff checks passed. R1 does not add migrations, dependencies or protocol discriminants. Independent R1 delta review accepted the frozen source with no further material findings. Root integrated Linux evidence follows below; R0 acceptance is not inferred.


Root verified all42 frozen author file hashes and the full patch hash before applying
the change. The integrated chat file retains the published reminder work. The only
product changes after that freeze simplify display copy and omit a zero unavailable
count; all literal rendering, original invocation, scope and history guards remain.
Final React settings4 checks and typechecked production build passed.

The first integrated Linux run passed native Google6 and actual Runtime Google4.
A broad Store preview filter passed the new authority/history/output cases but also
reached a stale community test that hardcoded six bundled profile pins. The actual
profile already contains seven, independently of this preview capability. Root
changed only that fixture to assert that installation preserves every bundled pin
and adds exactly one external pin, and removal restores exactly the original pins.
The failure is retained in evidence, not counted as an all-pass run.

Final integrated offline Linux acceptance passed: Store credential-chat5, the
repaired community lifecycle1, durable preview history with the feature disabled1,
preview HTTP1 and default-off HTTP1; affected four-package all-target Clippy with
warnings denied and the default assistant/native companion build also passed.
The capsule source hashes were rechecked. The earlier native6/Runtime4 results
remain valid for their unchanged protocol/control paths; these overlapping scopes
are not summed into an overall product-quality score. The final build embeds the
updated React output. No real Google account, credential or production service
was used, and CI remains disabled.
