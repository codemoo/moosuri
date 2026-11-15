# Live model evaluation setup result

A private synthetic eleven-case development corpus was prepared for memory
creation/correction/scoped forgetting, cross-thread recall, source-grounded totals,
instruction conflict and follow-up calculation. It uses the actual HTTP/SQLite
application and the existing official local Codex login, with GPT-6.1 Terra/high.
Credentials stay in the official runtime; apps, plugins, shell, hooks, memory and
MCP tools are disabled. No real mail, sources, browser or vault data are supplied.

The evaluated native binary contains the continuation42 implementation and the
reviewed installer changes. It excludes later invitation43, credential44 and
sharing changes. This result is not acceptance of those newer features or the
production personal-subscription adapter.

After resolving local sandbox startup, the official Codex0.168.0 runtime passed
the real text capability probe. The dynamic verification tool was called and its
supplied text was acknowledged successfully, but the runtime then emitted an empty
completed final answer. One bounded diagnostic retry reproduced that result. The
tool capability probe correctly failed; no database probe flag was bypassed and
no answer was synthesized to make the evaluation pass.

All eleven semantic cases therefore remain **not_run**. There is no new quality
score or production latency claim. The earlier development results remain separate;
this record preserves the failed setup rather than counting unexecuted work as
success. Private protocol diagnostics stay outside the public repository.

## 2026-11-15 protocol diagnosis

A new metadata-only, single-turn probe retained the same official Codex0.168.0,
GPT-6.1 Terra/high, disabled native tools and one synthetic dynamic tool. The
completed tool item acknowledged the supplied result. The matching final-answer
item still had zero text and no text deltas; the matching completed turn had
`itemsView: "notLoaded"` and no items. This establishes no recoverable text in
those events, rather than a successful answer or a shipped-provider defect.

The exact version's [official thread processor](https://github.com/openai/codex/blob/rust-v0.168.0/codex-rs/app-server/src/request_processors/thread_processor.rs)
explicitly rejects `includeTurns` in `load_live_thread_view` and
`thread/turns/list` in `load_thread_turns_list_history` for ephemeral threads.
Consequently, a proposed additional live history-hydration probe was not run:
the source already rules out that recovery path. Closing an ephemeral thread
does not leave persisted history to recover later.

The private evaluation adapter remains blocked pending an evidence-backed
official-runtime compatibility fix. No global CLI update, model/effort downgrade,
capability override, synthetic final answer or raw internal-event capture was
used. The eleven application scenarios remain **not_run**; compilation of the
current application does not change that result.
