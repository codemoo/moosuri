# Lock identity and local notice evidence

Fixed web/vault lock inputs define every per-app installation position, including nested/scoped packages, aliases, development and missing optional-platform packages. Strict position/name/version checks bind installed package metadata to that source inventory. Supplied roots must be real absolute directories. Source URLs are bounded/allowlisted but never fetched or emitted; SRI is source-lock metadata rather than verification of unpacked files.

Traverse bounded package directories without following observed symlinks, skip nested dependency trees, and read only package.json and conventional license/notice candidates. Safe SEE LICENSE IN references must remain candidate paths. Preserve notice bytes as exact UTF-8 text plus SHA256 and emit only selected metadata/hash fields, never arbitrary package configuration/scripts or host paths. Stable trusted inputs are required; this is not a hostile-filesystem sandbox.

Missing packages/declarations/notices/references and conflicting declared labels are explicit per position and aggregate counts. A strict option writes the same report then exits nonzero. Deterministic order/hashes aid review without treating candidate coverage, SPDX labels or registry integrity as legal clearance. Generated local reports remain outside Git and do not modify signed-release formats.
