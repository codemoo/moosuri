# Legacy v1 local import acceptance

The P8/F50 unit imports an explicit authenticated `musuri-vault` v1 file into the
currently unlocked v2 owner vault. The legacy password/recovery wrappers and
manifest/payload authentication remain distinct from the current archive format.
All payloads must pass before preview; selected items receive fresh destination
IDs and encryption. The original service, file and operational data are untouched.

An independent reviewer first extracted the old types/validation/engine contract
without viewing the implementation. Their later frozen review checked all18 file
hashes, exact legacy domains/canonical encoding/KDF/manifest, field preservation,
destination authority, Worker lifetime and exact receipt/resume/cancel behavior.
No material blocker was found. The actual historic libsodium build version remains
unknown; independently authored WebCrypto/PyNaCl synthetic vectors are the bounded
cross-runtime evidence, not a claim that real personal archives were exercised.

Native verification passed three legacy core cases, one WASM receipt case, the
affected current-archive receipt regression and four Worker cases; affected
all-target Clippy, actual wasm32 release/glue, React typecheck/build and strict
OpenSpec passed. Integrated Linux Rust1.89 ran the same three legacy core cases,
legacy WASM receipt and shared archive receipt case, affected all-target Clippy
and the vault-service build with the new embedded React/WASM assets. These counts
overlap and are not an assistant quality score.

Actual Chrome used the built React app and WASM Worker, a real retained Rust
ciphertext service/SQLite on loopback, and synthetic authority replies. The new
owner vault was created with a fresh local password and recovery verification.
Wrong source password produced no preview; the correct Unicode password imported
both independent synthetic entries. Unicode fields, secret reveal, working TOTP
and absent optional TOTP were preserved. Reload/unlock retained both entries, and
import via the old recovery key reported both as duplicates and disabled saving.
Observed POST bodies contained none of the synthetic source password, recovery
key, item secrets, notes or TOTP seeds. A premature browser assertion was corrected
to wait for the newly selected detail view; no product change or repeated passed
import was needed. The completed screen was visually inspected.

The browser storage binary is a retained previously accepted API build; the new
integrated Linux embedded-service build is separate build evidence. No real
operator export, production account/service, original collector/cron/DB, phone or
physical memory erasure was tested. Current256-item/26MiB and destination field
caps intentionally reject larger whole exports. Source completeness/latestness,
deployment and actual historical-file acceptance remain distinct gates.
