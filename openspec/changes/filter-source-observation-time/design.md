# Design

Use existing source-version `observed_at`, independently of `valid_from`/`valid_until` and `valid_at`. Apply observation bounds before both FTS and short-substring candidate enumeration. Start is inclusive; end is exclusive. Reuse the existing supported year1–9999 bound and strict start-before-end rule. Memory queries reject supplied nonnull bounds; no mixed query kind is added.

Optional fields default to absent and serialize only when present, retaining old query hashes and learned receipt payloads. Existing cursor signing binds serialized range, query, owner, time and knowledge generation. React uses device-local datetime inputs, preserves the searched range in pagination and invalidates pending search when fields change.

No learned route currently binds observation dates structurally. Both source-learning proof creation and evidence-calculation binding reject nonnull bounds. Existing unfiltered source route execution explicitly supplies absent fields, so old proofs remain usable. No tool/profile generation bump is needed for these compatible optional read arguments.
