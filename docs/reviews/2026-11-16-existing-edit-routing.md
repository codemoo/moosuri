# Existing document edit routing

Baseline: `9d72356ab0081c4440964e4d25284e76cfaeafbc`.

The read-only audit found two material paths:

1. `artifact_intent::plan` skipped its classifier whenever
   `compound_report_candidate` saw document + read + save cues. An exact named
   existing-document edit therefore acquired a permanently read-only artifact
   profile; the mission domain explicitly excludes existing-document edits and
   cannot return to artifact admission.
2. Artifact target resolution returned a composer pin before checking a provided
   current literal target. Selecting A while naming B could seal an edit of A.

The bounded repair keeps explicit edit verbs outside quoted literals in the
artifact route and checks selection/name agreement before sealing a grant.
Selected null/deictic references retain their existing target identity.

Native Rust1.89 verification used the isolated shared build directory, offline
locked dependencies and synthetic temporary stores:

- Baseline core regression failed on a named existing-document read/edit/save
  request being incorrectly classified as a fresh compound report.
- Baseline runner regressions failed because the exact-named edit completed
  without a stored receipt and because selected A/current named B wrongly saved
  a document. Matching/null/deictic selected edits already passed.
- `cargo test --offline --locked -p moosuri-core compound_report_tests -- --nocapture`
  passed all four tests after the repair. Preservation cases include quoted edit
  words in titles, revised input documents, fresh reports, software-update topics
  and calculations of the correct total.
- `cargo test --offline --locked -p moosuri-agent artifact_tools::tests -- --nocapture`
  passed all eleven tests. Actual runner/SQLite outcomes cover same-ID revision2
  save, both documents unchanged on a target conflict, selected matching/null/
  deictic edits, receipt replay, CAS, withdrawal and external-read report storage.

The finite cue helper remains a Korean/English cost gate; isolated model intent
and Store validation still grant authority. These checks claim plumbing and
admission behavior, not live model semantics. Independent authority review and root integration are complete. The repeated
quoted-name correction below also passed separate frozen review.

Independent review found a repeated-quote bypass in selected-target admission:
`'그 문서' ... '그 문서'` made the recent-receipt helper's single-occurrence
predicate false and was wrongly accepted as a deictic selected reference. The
selected branch now detects any quoted occurrence independently of occurrence
count. The existing recent-receipt ambiguity rule is unchanged. A focused actual
runtime regression checks that both selected A and literal-title B remain at
revision1 with their original bodies.

The new regression failed on production `24a5380` at the no-save-receipt
assertion, then passed with the follow-up. The focused `artifact_tools::tests`
suite passed all12 tests, including matching/null/plain-deictic controls and
the existing conflict, CAS and receipt outcomes. Baseline and candidate logs are
`/private/tmp/moosuri-repeated-quote-baseline.log` and
`/private/tmp/moosuri-repeated-quote-candidate.log`; no live data was used.
