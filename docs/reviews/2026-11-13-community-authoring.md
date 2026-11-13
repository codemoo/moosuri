# Native community authoring — implementation and integrated acceptance

Date: 2026-11-13. Scope: local `skills init` and `skills check`, based on the published schema13 source. No dependency, schema, network/model, installation or Git action is included. The isolated implementation was subsequently integrated with automatic-memory14/destructive-intents15; integration evidence is recorded below.

## Behavior and compatibility

The command branches before installation/default data-dir handling. Init creates exactly SKILL.md/moosuri.toml/examples.json/LICENSE from an original synthetic saved-source procedure, normal/unsupported examples, existing knowledge capabilities and the full repository Apache-2.0 text. It does not inspect an installed package or private data.

The pure public `skill_author` facade uses the same content validator as curated and GitHub installation. The shared refactor changes only diagnostic representation; installer callers still receive InvalidInput and the digest framing remains exact. Catalog validation continues to compare all original four-file bytes against the existing pin manifest. Check reports compatibility rather than safety, license-rights or task-quality certification.

Existing parent directories are opened component-by-component with nofollow directory descriptors and checked for installation-data markers. Normal repository markers are allowed. Parent traversal, symlink components and existing destinations reject. Four regular nonexecutable single-link UTF-8 files are read from the pinned directory descriptor with 32KiB/file and64KiB aggregate bounds; unknown/missing files, bad content and special modes reject without raw content output.

Init creates an owned private sibling staging directory, creates and fsyncs its four nonexecutable files, syncs that directory and uses native Linux/macOS no-replace rename before syncing the parent. Even an empty existing destination cannot be replaced. Cleanup removes only tracked same-inode created files, never recursively deletes, and preserves unknown contents or replaced directory paths. After a successful rename followed by a parent-sync error, a complete destination can already exist; the CLI returns failure and documentation tells the contributor to inspect/check it. Interrupted-process stale partial-directory GC remains outside scope.

## Synthetic verification

Focused results and exact commands/log hashes are recorded in the sibling delivery JSON. macOS focused Store community regression:6 passed. Actual author CLI:5 passed. Native CLI unit tests:4 passed, including owned cleanup. Existing backup CLI:2 passed,1 explicitly ignored reference interoperability case. Affected all-target clippy with warnings denied, fmt, plan348 local links and strict change validation pass; root integrated/Linux checks remain pending. Tests use synthetic temporary trees only; no Store is opened for authoring. Actual-process cases cover init→check, default relative/custom repository destination, absent installation, malformed/unknown metadata, NUL/UTF-8 and per-file/aggregate limits, extra/missing/link/hardlink/executable/socket/directory entries, traversal/missing parent/app-data rejection, and simultaneous init with one complete winner and no partial debris. A focused cleanup case checks unknown content and replacement-path preservation. Pure facade cases verify installer parity and exact curated pins. Existing backup CLI routing is checked separately.

Web assets are built locally before compiling the embedded CLI. Only affected Rust tests and warnings-denied all-target clippy, fmt, plan links and strict OpenSpec checks are required here; no broad duplicate workspace suite or live semantic evaluation is claimed. Initial attempts exposed environment temporary-directory/toolchain configuration and a macOS variadic-mode cast compile issue; final evidence is kept separately after correction.

## Remaining work

Independent review found no material defects in the frozen author path and shared content validator. The author decides what to edit, review, commit and publish. Existing UI installation still pins immutable public GitHub source under current account authority and grants no new native tools. Release/catalog generation, private-learning export, local agent edits, installed-snapshot mutation, scripts, runtime tasks, automatic publishing and GC remain unimplemented.

## Integrated acceptance

The merged macOS CLI5 process tests passed. Linux x86_64 with Rust1.89 passed
Store3 focused content/author cases and CLI13 cases, with one separately opt-in
reference interoperability case ignored. The CLI cases include native no-replace
publication, competing init, cleanup, keygen/backup routing, account recovery and
embedded-web installation/restart. Source/vendor mounts were verified and read-only;
the container was nonroot, network-disabled and removed after each phase. No
production directory or service was changed. Exact source hashes matched the
snapshot at completion. The dependency files were unchanged by this unit.

The root retained the newer schema15 startup, automatic-memory and destructive
intent recovery paths while adding the pure author facade. There is no schema or
DTO change. The independent source/filesystem review and separate synthetic
init→check→no-overwrite smoke passed. Power-loss/syscall-fault and live task-quality
results are not claimed.
