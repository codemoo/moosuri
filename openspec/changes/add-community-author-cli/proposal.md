## Why

R26/R27 and F67 describe a small contributor path, but authors currently hand-copy the four-file compatibility shape. A native template/check command makes that path concrete without adding installation or publishing authority.

## What Changes

- Add `moosuri skills init <slug> [--output DIRECTORY]` with original synthetic examples and Apache-2.0 license text, published only to a new directory.
- Add `moosuri skills check DIRECTORY` using the existing installer content validator, with bounded nofollow local reads and useful content-free diagnostics.
- Document the contributor-controlled edit/review/commit/PR path and existing immutable GitHub installation flow.

## Impact

CLI dispatch and a pure Store compatibility facade; no migration, dependency, network, model call, installation or runtime authority. Existing curated package bytes/pins, GitHub envelopes, native backup commands and data-dir handling for other commands remain compatible. Release/catalog generation, private-learning export, installed-package editing and GC stay outside this change.
