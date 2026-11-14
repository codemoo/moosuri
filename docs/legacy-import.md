# Private normalized export import

This development CLI imports one explicitly selected normalized private transfer
bundle into a **new** SQLite staging installation. It never opens a legacy DB,
executes legacy SQL, contacts a provider, changes a collector/cron/service or performs
production cutover. A normalized bundle must be prepared and reviewed separately;
no raw PostgreSQL dump, existing deployment schema or automatic legacy exporter
adapter is implemented here. Do not infer migration parity from a successful import.

## Run privately

Keep the original bundle and fresh password file outside Git, served directories
and both output roots. Both files must be owned by the current OS user,0600 regular
files with one link and no symlink. Bundle limit is16MiB/512events. The password
is a newly selected local enrollment credential, never taken from the legacy bundle.

```sh
moosuri --data-dir /absolute/private/new-assistant-data import-private \
  --bundle /absolute/private/transfer/export.json \
  --legacy-owner EXACT_SELECTED_OWNER \
  --name 'New staging owner' --email new-owner@example.test \
  --password-file /absolute/private/keys/new-password \
  --report-dir /absolute/private/new-import-report
```

Both destinations must be absent beneath existing private parents, disjoint and
outside known source/installation/package roots. Final data paths and ancestors
using the reserved `.moosuri-restore-stage-*` prefix reject before staging so a
successful output can pass ordinary runtime admission. The selected legacy owner maps
only to this newly enrolled admin/tenant. A later invited user inherits none of
these rows. The command prints only new account UUID and event counts; no private
owner identifier, source text, password, raw rejected row or database error text is
printed. The account uses the supplied new password. No legacy role, password hash,
session, OAuth, provider/broker binding, Push state, budget, task, schedule, receipt
or execution checkpoint is admitted.

The implementation builds under the existing blocked private restore-stage prefix
using a fresh internal admission token. Ordinary serve/recovery cannot open an
interrupted partial stage. All actual data operations use the normal authenticated
Store APIs and destructive journals. After imports, source collections are
explicitly disconnected, temporary import login is revoked, integrity/FKs checked
and files synced. Reports publish first in their own new directory; the complete
SQLite root publishes last by no-overwrite rename and parent-directory fsync.
A report left by a later data publication failure is an orphan report, not proof
of completion. Existing destinations are rejected; retries do not rewind account
or imported data. Choose new output paths after examining a failed first attempt.

## Explicit transfer contract

The input is UTF-8 JSON with exactly these top-level fields:

```json
{
  "format": 1,
  "purpose": "moosuri-private-transfer-v1",
  "bundle_id": "00000000-0000-4000-a000-000000000001",
  "source_manifest": {
    "deployment_id": "synthetic-legacy",
    "source_version": "reviewed-export-version",
    "source_schema": "reviewed-schema-contract",
    "exported_at": 1,
    "coverage": "selected ordered source/memory/document history; no authority"
  },
  "events": [
    {
      "sequence": 1,
      "owner": "selected-owner",
      "id": "legacy-memory-1",
      "revision": 1,
      "operation": {
        "kind": "memory_upsert",
        "subject": "Synthetic assertion",
        "text": "An explicitly transferred private fact",
        "speaker": null,
        "valid_from": null,
        "valid_until": null,
        "evidence": []
      }
    }
  ]
}
```

`source_manifest` is imported evidence supplied by the exporter, not an authenticated
provider receipt, latestness proof or proof that every original row was exported.
Keep the separately reviewed source/export manifest and original unchanged bundle.
No existing legacy source is guessed or uploaded by this tool.

Events must be ordered with sequence1..N; order is not inferred from timestamps.
Each selected-owner object has an independent revision1..N history, maximum128
content revisions and optional terminal deletion/forget. IDs are nonempty opaque
labels up to128bytes; they do not grant account access and map to new target UUIDs.
A source ID's scope and external ID cannot change across revisions, and distinct
legacy IDs cannot alias the same external ID within a scope. Original source
external ID/version, speaker, observed/valid times and complete supported source
version history enter ordinary knowledge rows. Imported scope and identity mapping
remain in reconciliation reports; collection scope uses a private import namespace.
Memory/document creation/update timestamps are import-time Store timestamps; only
explicit source observed time and supplied validity intervals are preserved.
No connected provider provenance is invented: these are disconnected manual source
collections, and the normal source `provenance` field stays absent.

| Operation kind | Required operation fields |
| --- | --- |
| `source_upsert` | `scope`, `external_id`, `version`, `title`, `text`, `speaker`, `observed_at`, `valid_from`, `valid_until` |
| `source_delete` | none |
| `memory_upsert` | `subject`, `text`, `speaker`, `valid_from`, `valid_until`, `evidence` |
| `memory_forget` | none |
| `document_upsert` | `title`, `body`, `references` |
| `document_delete` | none |

Nullable fields may be JSON null. Source content/version/time, memory drafts and
plain documents obey existing Store bounds. Source versions must change when a
new revision is declared. Memories support explicit corrections. An evidence entry
is `{source,revision,start_byte,end_byte,quote}` and must exactly match the current
mapped source revision and UTF-8 byte range at that event's position in the ordered
stream. A document reference is `{kind,id,revision}`, where kind is `source`,
`memory` or `document`, and must be currently valid at that position. This ordering
preserves historical evidence through the actual normal mutation rules, including
invalidation when a later source changes and withdrawal of dependent documents.
References to later, missing, retired, invalidated or mismatched objects quarantine.
Only plain documents are supported; goals/followups and their authority are not
silently reduced to documents.

First revision1 deletion/forget can represent an already withdrawn object whose
body was not retained in the export. It preserves an imported mapping barrier
without fabricating target text. Once an object is retired or quarantined, later
operations cannot resurrect it. A failed correction also withdraws its already
staged prior content before publication; a stale old assertion is not exposed as
if its correction succeeded. Reintroducing an invalidated/withdrawn document as a
new current revision is not guessed around the normal Store authority rules.
The event quarantines and its previous versions remain withdrawn/deleted.

## Reconciliation and quarantine

Every input event receives a SHA256 of its parsed canonical JSON serialization (the
whole bundle SHA256 covers the exact original bytes), sequence/index, recognized category and
applied/quarantined outcome with a fixed reason code. Other owners, unknown
operations, extra/invalid fields, invalid sequence/revision/content and unresolved
references are accounted for. System/IO/integrity failures abort publication rather
than being mislabeled as a row rejection. Successful earlier events may later be
withdrawn; final mappings record the resulting state and target revision.

The private report directory contains `report.json` and `quarantine.json`.
Quarantine contains hashes/positions/reasons, not a second plaintext copy of raw
unknown credential/run payloads. The original selected bundle remains unchanged
and is required to inspect or repair rejected rows. No rejected row is quietly
reported as imported. Loss of the original bundle loses those rejected payloads;
normal application backup does not reconstruct them.

Bounded report/mapping/outcome chunks are also stored as ordinary private document
artifacts in the new SQLite DB, so existing encrypted exports preserve them.
Those are editable imported reports, not a new immutable audit authority. Sibling
reports and the original bundle are outside the DB backup inventory and must be
retained separately. Editing/deleting a report artifact changes what later backups
contain; preserve the original sibling reports for reconciliation evidence.

Old billing/usage, execution/receipts, connector cursors, settings, learned proof,
private media, attachments, vault ciphertext/nonce/AAD and credentials are not
converted. Corresponding events quarantine with the original bundle retained.
New installation usage is not a claim that the legacy deployment's spending was
zero or reconciled. There is no production read comparison, drain, source writer
handoff, rollback or cutover in this unit. Use [migration](migration.md) and
[historical restore](historical-restore.md) for those remaining boundaries.

## Source-grounded schema23 input

The [relation adapter](legacy-relations.md) supplies the normalized input directly
from a complete explicit schema23 PostgreSQL relation snapshot. It imports current
projections with exact joins and full row accounting, without inventing missing
revision history. The SQL export recipe requires an isolated operator-managed
snapshot; production execution, media/vault and cutover remain separate.
