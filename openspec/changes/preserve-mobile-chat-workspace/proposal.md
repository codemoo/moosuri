# Preserve the mobile chat workspace

## Why

Mobile keyboard and composer size changes can hide the conversation or move the
reading position. Separate idle media controls consume unnecessary vertical space;
a long desktop menu places account controls outside the visible frame.

## What changes

Size the authenticated workspace to the visual viewport at normal zoom, retain
pinch zoom, preserve bottom or reading anchors on asynchronous layout, and place
the new-response control above the actual composer. Keep idle attachment/voice/
library controls compact and allow expanded tools and navigation to scroll.

## Impact

React/CSS only. No model, API, storage, upload, recording or authentication changes.
Actual mobile OS keyboard/device acceptance remains separate from Chrome layout checks.
