# Unify received model round persistence

## Why

Foreground chat, isolated planners and Claude already retry a local checkpoint on
Store Busy. The mission runner does not, so brief writer pressure discards an
otherwise valid received round and ends the task. Four separate implementations
allow this policy to drift.

## What Changes

Use one agent-owned commit function for the same lease, response bytes and tool
slots. Apply the existing bounded Busy-only policy to all four callers, preserving
each caller's terminal/error handling and notification order. Do not retry model
dispatch, tool execution or unknown acknowledgments.

## Impact

Agent execution and recovery only. No API, schema, new model call, provider change
or user confirmation. Automatic CI remains disabled.
