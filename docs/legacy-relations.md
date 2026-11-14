# Legacy schema23 relation import

`moosuri import-legacy-relations` adapts the actual schema23 PostgreSQL relation
shape into the existing private importer. It never connects to PostgreSQL, opens
old credentials, executes the old runtime, or changes the original service.

The old backup format is an age-encrypted custom `pg_dump`, not JSON. An operator
must first restore and replay that backup using the original recovery procedure
into an explicitly isolated PostgreSQL snapshot. The product-authored
[`schema23-relations.sql`](../tools/legacy/schema23-relations.sql) then exports all
non-system base/partition relations in one repeatable-read, read-only transaction.
It uses `FROM ONLY` per relation, inventories even unknown tables, and closes with
the same transaction-snapshot marker. Use `psql -X --no-password`, a private output
directory and `umask 077`; provide connection credentials outside command arguments.
The SQL recipe has not been executed against a real legacy database in this change.

```sh
moosuri --data-dir /absolute/private/new-data import-legacy-relations \
  --snapshot /absolute/private/schema23.jsonl --legacy-owner chosen-owner \
  --name 'Fresh owner' --email owner@example.test \
  --password-file /absolute/private/fresh-password \
  --report-dir /absolute/private/new-reports
```

The legacy database has one global owner and no tenant/owner row column. The
explicit `legacy-owner` binds the selected isolated snapshot to a fresh account;
it does not import an old password, session, provider, connector, schedule, work
queue, approval or execution capability. Both output paths must be new. Existing
outputs are rejected. Input and password files must meet the existing private-file
checks. The unchanged original input remains the operator's private evidence.

The adapter requires all 66 schema23 public relation names and the exact 23
migration filename/checksum pairs recorded in `tools/legacy`. Unknown relations
are counted and quarantined. Every original row has relation/index, canonical JSON
SHA256, decision/reason and approved structural metadata in the reconciliation
report. Raw unsupported payloads are not copied into the database, report or logs.
These checks bind an explicit source contract; they do not establish snapshot
latestness, provider authenticity or production parity.

Only current source documents, current memories and plain plan/note/reading
artifacts are eligible. Sources retain the supplied current version and observation
time in disconnected collections, with no invented provider receipt. Memories
require matching current claim, item projection and unsuppressed current identity.
Source-origin memory requires an active support matching the exact admitted source
binding/key/version and one unique UTF8 quote span. Artifact references require the
exact current source version or memory/artifact revision, valid visibility epoch
when supplied, and bounded acyclic dependencies. Invalid rows quarantine.

The old relations contain current projections, not previous revision bodies.
Legacy revision N therefore becomes new revision 1; N is retained as report
metadata. No prior value or history is invented. Deleted/forgotten items become
bodyless withdrawal records. Opaque old identity suppression cannot become a native
new identity barrier: any suppressed alias, deleted or withdrawn memory withholds
every implicated binding/key source from admission. Missing support scope withholds
all sources. A current owner correction can survive an old suppressed alias only
with its valid current projection and without invalid active supports. This policy
prevents later extraction from admitted old raw sources from reviving withheld
identities; the report does not claim to import native suppression semantics.

Inputs over 16 MiB, over 4096 original rows, or over 128 converted objects reject
as a whole. There is no truncation. At most 32 source scopes and 512 generated
transfer events are admitted. Reports are chunked below document limits. The
accepted importer applies normal Store validation and refuses the complete adapter
import if any generated event would quarantine. Reports are staged before new data
publication; `legacy-adapter-report.json` is included in the same private sibling
report publication. Bounded reconciliation documents are also stored in SQLite
and included in normal encrypted backups. The original input and sibling report
directory require separate private retention; they are not copied wholesale into
SQLite backup scope.

Source evidence was inspected only in local migration SQL and selected
`memory.py`, `models.py`, `workspace_tools.py`, `recovery.py`, `migrations.py` and
source modules. No private source code or real data was copied into this product.
Synthetic checks cover real Store/import/age backup and actual CLI use. Operator
restore/export verification, production comparison, media/vault migration and
cutover/rollback remain open aggregate migration gates.
