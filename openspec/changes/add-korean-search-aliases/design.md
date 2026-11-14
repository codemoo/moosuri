# Design

`match_mode` is absent by default; absent/null/explicit literal omit its serialized field. `korean_aliases_v1` remains serialized and therefore bound to signed cursors. A closed enum rejects unknown versions. Existing omitted-field receipts, cursor hashes and source-route inputs retain their old shape.

The immutable dictionary contains 휴대전화/휴대폰/핸드폰, 전자우편/이메일 and 비밀번호/패스워드. Whole normalized family queries expand, otherwise exactly one raw whitespace token must be a family member. Zero/multiple matches fall back literal; punctuation and inflection are not normalized into family tokens. Substitute only that token and preserve all other normalized query text and order. No identity filters change.

All expanded variants contain at least three characters and use a bounded FTS OR expression. Existing current/eligible source or memory rows use membership in that result before one stable ID LIMIT/OFFSET page, deduplicating overlapping aliases. Short unknown/fallback queries retain the previous bounded substring path. Tenant, collection, subject, speaker, validity, observed-time and generation/deletion predicates are shared unchanged.

Alias-mode source/evidence proofs are refused because current routes do not structurally bind a fresh match mode. Literal remains compatible and generation3 remains unchanged. A public hand-authored fixture fixes source/query ID sets before the implementation; real Store retrieval compares both modes against those sets. Separate HTTP pagination/lifetime, native parser/legacy serialization, learning and React outcomes cover the integration boundaries.
