## Why

The private importer accepts explicit normalized events but cannot read a legacy
custom pg_dump or prove the actual schema23 relation projection. A separate bounded
adapter must expose the current-only semantics and account for unsupported rows.

## What changes

Add a read-only isolated-snapshot SQL export recipe, exact schema23 contract data,
a local relation JSONL adapter and an actual CLI path into fresh private import.
Opaque suppression withholds raw sources, current revisions map explicitly to one,
and every row receives a reconciliation outcome. No old execution authority is
admitted. Real operator restore/export comparison remains a separate gate.
