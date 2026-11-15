# Accept equivalent exact decimal answer formatting

A real-model synthetic private-learning lifecycle returned canonical exact tool
results `140` and `38.6`, then correctly presented `140.00` and `38.60` in its
final answers. Literal token equality rejected both observations. Repair this
bounded answer-adoption contract for F26/P4 private learning.

Normalize both the receipt number and an entire supported answer number using
exact decimal strings: optional plus, leading integer zeros and trailing
fractional zeros do not change the value. Preserve nonzero signs and every
significant digit, including workflow precision and noncanonical source literals.
Bound supported tokens to128bytes. Keep ordinary currency prefixes, Korean
unit/sentence suffixes, Markdown emphasis/code and sentence punctuation. Reject
numeric components in grouping, exponents, percentages, expressions, identifiers,
versions and malformed multi-sign/multi-dot forms rather than treating their
fragments as adoption. This deliberately tightens the former permissive split
around unsupported separators.

No floating point, tool input changes, schema/generation reset, new authority,
proof canonical-input changes or automatic sharing. Existing receipt validation,
owner/finalization/dependency fences and two-distinct-input readiness remain.
This repair does not certify semantic answer quality or unsupported number forms.
