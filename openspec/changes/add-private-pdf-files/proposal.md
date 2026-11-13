## Why

Owners need to attach ordinary PDF documents without blocking message composition or relying on a Python/system PDF installation. Existing private file ownership, retry, deletion and untrusted-context behavior must apply.

## What Changes

- Add bounded pure Rust text extraction in a fixed, resource-limited subprocess.
- Preserve original PDF bytes/digest; present extracted page text in authenticated preview and chat source context.
- Explain unreadable/encrypted/scanned/over-limit failures and retain the original upload for explicit retry.

## Impact

New Rust document crate and one pinned parser dependency; CLI hidden child entry point, authenticated file route, existing Store media/file storage, React attachment/library. No new schema or tool/write authority. Linux/process/ownership review and real browser acceptance precede completion; OCR and speech remain separate.
