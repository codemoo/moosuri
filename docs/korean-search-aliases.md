# Fixed Korean aliases in saved-knowledge search

Open **검색 조건 → 한국어 별칭도 찾기** to search these public word families together:

- 휴대전화 / 휴대폰 / 핸드폰
- 전자우편 / 이메일
- 비밀번호 / 패스워드

The default stays literal. Alias mode applies when the complete whitespace-normalized query is exactly a listed word, or when exactly one raw whitespace-delimited query token is a listed word. For example, `분실 휴대폰 보험` also looks for `분실 휴대전화 보험` and `분실 핸드폰 보험`. Other words stay in their original order, and the established normalized whole-query substring semantics remain. `분실 휴대폰 추가 보험` does not satisfy `분실 휴대폰 보험`.

A query with two matching tokens, including two in the same family, falls back to literal matching. Unknown words, inflected family tokens such as `휴대폰을` and punctuated tokens such as `휴대폰,` are not rewritten. No typo, name or morphology inference occurs. Exact collection/subject/speaker filters remain unchanged. Search text can use this mode for sources or memories; each kind retains its existing time and lifetime predicates.

`POST /api/v1/knowledge/search` and native `knowledge_search` accept optional `match_mode`: absent/null/`literal` keeps the prior serialized query and results; `korean_aliases_v1` selects the fixed dictionary. Unknown versions are rejected. At most three complete query variants form a union before stable ID pagination; overlapping terms return a source/revision once. Carry the exact mode, text, scope and dates with a returned cursor. Owner, validity, observation-time, deletion and knowledge-generation fences still apply.

Alias-mode calls are ineligible for reusable source-learning proofs or evidence-calculation bindings, including literal fallbacks. Existing routes have no structural matching-mode parameter, so such reads cannot silently qualify an older route or export a fixed expanded query. Ordinary literal routes and generation3 proofs retain their existing contract. No SQL migration, dependency, capability-generation bump, model or embedding service is added.

The public hand-authored [fixed gold fixture](../crates/moosuri-store/src/knowledge/fixtures/korean_aliases_v1.json) declares source IDs, query IDs and expected literal/alias sets. It includes phrase and hard-negative cases and is a bounded development comparison, not an independent language-quality benchmark. See [verification](reviews/2026-11-15-korean-search-aliases.md).
