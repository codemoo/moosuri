## Why

R26/R27, F67 and community task2.5 need reproducible release artifacts from public source without allowing local bytes to claim a Git commit or exposing private learning.

## What Changes

- Add typed read-only catalog generation and `moosuri skills catalog` for an exact public GitHub commit and explicitly declared package directories.
- Reuse verified object acquisition, four-file installer validation and existing external v2 envelopes/digests; generate a deterministic static index with separate artifact byte digests.
- Atomically publish a complete local new directory without overwriting existing output.

## Impact

No migrations, dependency changes, Git subprocess, hooks, private export, automatic publishing, runtime catalog consumer or activation. Existing curated v1 hashes/pins and installation formats remain unchanged.
