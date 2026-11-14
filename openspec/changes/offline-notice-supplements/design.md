# Design

A shared TOML reader validates bounded explicit real manifest paths, strict keys, unique ecosystem/name/version/lock-digest identities, reviewed HTTPS repository/full commit/source paths, provenance kind and exact inline UTF-8 notice hashes. Matching comes from the existing collectors' lock-bound component rows. All entries validate before report mutation; repeated npm positions match only exact identity/SRI.

Supplemental evidence/coverage is separate. Original fields and `--require-complete` read the same local missing count; no unverified notice can change that gate. Provenance routes stay operator assertions with authenticity false. Inline text avoids filesystem selectors, extra source-tree scans and network access. Optional line ranges describe exact notice-header fragments without copying source code into Git. Existing command/report/release formats remain usable without this optional input.
