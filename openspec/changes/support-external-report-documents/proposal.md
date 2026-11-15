# External reads into durable private reports

## Why

A user can already research public HTTP, connected Gmail and primary Calendar in
one conversation, but cannot save that result as a document with safe withdrawal.
A real report needs a durable version and exact source receipts, not a save claim.

## What Changes

Schema54 pins additive external-report capability only for newly admitted tool
runs. One isolated current-user document create/edit grant can use configured
public HTTP and the owner's existing pinned Gmail/primary Calendar reads. The
Store derives exact private request/result receipt provenance, persists it with
the document mutation and exposes inert metadata in history/exports/editor.
Selected same-conversation completed reads can ground a subsequent explicit save.
Connection, evidence, source-run and conversation withdrawal retire derived
versions/answers; restore conservatively withdraws external-derived documents.

## Impact

F15/F21/F24/F27, P3/P4. Existing single-record grant, anchor/CAS, budget, atomic
confirmation and destructive-journal contracts remain. No additional approval,
new dependency, mail draft/send, Calendar action, browser/native-preview/recall
persistence, external-source automatic refresh or claim of semantic correctness.
