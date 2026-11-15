# Gmail attachment/PDF reads (F34)

This bounded slice replaces interactive/source raw-message body reads with the
official full MIME payload and adds exact per-message part reads. Large separately
stored PDFs no longer consume the body raw-MIME budget. The authenticated mail UI
and read-only model tools expose metadata and PDF/UTF-8 text excerpts, not attachment
sending, arbitrary downloads, OCR or general document execution.

Attachment text uses the existing deployment-owned moosuri-documents child, now
propagated from Config::start_runtime through the Runtime mail service. No parser
path is accepted from the user/model. Existing2MiB/50page/24KiB text resource limits
are retained. Binary and extracted-text SHA256 are both included; the continuation
digest binds both plus the exact message/part/attachment tuple. Serialized receipts
are capped at12KiB including escaping below the Store's existing16KiB cap. Status,
partial and next offset distinguish successful excerpt coverage from unsupported
or failed extraction. New MailToolRequest variants reuse existing external receipt
admission, accounting and mail-epoch revocation; no schema migration is added.
Only the already locked local moosuri-documents dependency edge is added to agent.

Full messages stay bounded to1MiB HTTP JSON,64 unique MIME parts, depth8 and64KiB
aggregate headers. Selected external text body parts are hydrated through the
same official API under128KiB aggregate /4-call bounds, independently of file
attachments. Alternative body formats are not duplicated. Metadata listing and
text pagination remain bounded. The separate attachment response supports decoded
bytes up to2MiB; this does not imply that every large inline attachment fits the
full-message JSON bound. Bodies over declared bounds return partial or explicit
failure, and source sync never commits truncated body text as complete.

Source sync remains account/label/time scoped body indexing using its existing
SourceIngestCapability and durable owner credential, without a fabricated session
or OAuth policy expansion. It does not download file attachments or index PDF text.
Interactive attachment results are revocable external receipts, not new source
objects. Durable connected missions require a freshly observed message and use the
actual owner grant after interactive logout. Disconnect fences late IO and purges
existing derived answers. UI scope changes drop late attachment responses.

Native Rust1.89 offline/locked focused checks passed on the task source:

- Connector attachment4: exact IDs,1.5MiB external attachment bytes, inline metadata,
  MIME alternative/depth/duplicate checks, external body hydration/call limit and
  body-only source collection beside a large external PDF.
- Connector Gmail6: body projection plus backfill/history paging, accepted labels,
  deletion/scope departure, expired history and identity boundaries.
  One source case overlaps the attachment filter; these are9 unique cases.
- Agent attachment4 including the explicitly enabled real PDF test: actual600KiB+
  PDF parsed through Runtime configuration and the real fixed binary child;
  extractor-unavailable/no-text/malformed status, current OAuth preserved; text
  paging, content-change restart, JSON envelope size, actual durable Store receipt,
  repeated receipt without IO, disconnect purge and late-IO fencing, unsupported
  metadata without binary download, forged URL field and missing digest rejection.
- Store connected-mission attachment1: invented message blocked, successful search
  evidence admits real owner grant after logout, disconnect fences pending result.
- UI3: exact tuple/digest continuation, explicit failed extraction, disabled
  unsupported format and late response suppression after mailbox scope changes.
- Web typecheck and production build; connector/agent/server/Store all-target Clippy
  with `-D warnings`; workspace formatting and diff checks.

The real-child test is ignored in default Cargo invocation because library tests
cannot select the deployment executable automatically. It was explicitly enabled
and passed, with **no ignored cases in that run**, using:

```sh
MOOSURI_TEST_PDF_EXECUTABLE=/absolute/path/to/target/debug/moosuri \
  cargo test --offline --locked -p moosuri-agent gmail_attachment_ \
  -- --include-ignored --test-threads=1
```

Integration must build its actual moosuri executable first, set the environment
variable to that executable and run the filter with include-ignored. A missing
variable fails the enabled case rather than silently substituting extraction text.
No executable temporary scripts are needed; `/tmp` noexec is compatible. Native
used the existing built binary whose hash is recorded in the freeze manifest;
moosuri-documents itself is unchanged. Final Linux and independent-review acceptance are recorded below. No account-backed Gmail/provider or production IO
was used; all protocol IO was loopback and all content synthetic. Public official
reference fetches are linked in the change design.


## R1: whole body-read envelope

Independent R0 review identified an aggregate result bug: attachment metadata was
limited but the separate attachment_names list, display headers and escaped text
could still exceed the Store16KiB receipt cap, especially for3-message threads.
R1 bounds the entire model-only read result to12KiB. Attachment names now mirror
only exposed metadata, and hidden metadata keeps an exact next-list index,
including0 when the shared thread budget cannot expose an attachment. Header and
recipient/label lists and body text shrink with explicit partial/truncation flags.
Exact provider message/part/attachment IDs are retained whenever metadata is shown;
the authenticated user's fuller REST/UI projection remains separate.

The focused regression passes through real MailService IO, durable Store admission
and complete_external_tool, rather than only measuring an isolated serializer.
A valid64-part MIME tree includes62 attachments with256-byte escaped filenames,
2048-byte attachment IDs, a heavily escaped body,2048-byte subject, long RFC message
header,100 addresses in each recipient/sender list and128 maximum-size labels.
Single-message and3-message thread receipts persist as External within12KiB, with
matching exposed names and exact remaining-metadata index. Metadata-list pages
also persist within the envelope and retain complete identifiers.

The UI now displays an in-flight reading status and human KB/MB sizes. Its existing
scope fence suppresses both the old result and loading status on mailbox changes.
R1 affected checks: dense Store-backed agent outcome1 and UI3/typecheck passed;
scoped agent all-target Clippy, workspace format and diff checks passed. R0 packet
is preserved. Previously passing unrelated connector, PDF and authority suites are
not rerun for this projection-only repair. Independent R1 re-review accepted the complete envelope repair with no new
material findings.

## Integrated Linux acceptance

The integration owner verified all31 R0 file hashes, then all nine R1 delta paths
against their frozen manifests. Independent review accepted the repaired whole
message/thread envelope and retained identifier/continuation behavior. The final
React production build and strict bounded OpenSpec validation passed.

The exact integrated source passed six offline Rust1.89 Linux phases: first build
the actual embedded assistant CLI, connector attachment4, connector Gmail6 (one
overlap, nine unique connector outcomes), connected-mission Store1, MailService/
Runtime5 with the actual PDF child explicitly enabled and zero ignored cases, and
connector/agent/server/Store all-target Clippy with `-D warnings`. The real child
path was `/target/debug/moosuri`, built in that first phase, with no executable
temporary script. The new dense result persists through the real Store in the same
Runtime filter. Source hashes still matched after all phases; archive SHA256 was
`614c7fc6fe763e9c0c869c08b6ecc7df3ddc5052bde99dc8c84855a2f184c2af`.

These are actual synthetic IO/storage/runtime/UI and Linux execution results, not
account-backed Gmail, independent semantic-quality or production acceptance. The
existing schema52, final mail-send approval and source body-only indexing contract
remain. No CI setting, real account, collector or production service changed.
