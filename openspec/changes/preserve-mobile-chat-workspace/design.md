# Layout ownership

The workspace hook observes normal-scale visual viewport resize/scroll and releases
listeners on unmount. Pinch zoom retains the last normal layout size; browser zoom
is never disabled. Browsers without VisualViewport keep the CSS dynamic viewport.

The chat scroll container and turn content share a ResizeObserver. A bottom-following
reader remains at the latest message; an older reader keeps the first visible run
and its relative top. Geometry changes are not interpreted as voluntary scrolls.
Existing older-page prepend restoration remains authoritative. The new-response
button uses actual composer/connection height rather than a fixed mobile offset.

Idle controls retain accessible names with short visible labels. Expanded recording,
sharing and library content uses the full composer width; the composer can scroll
within a bounded share of the panel. Desktop navigation also scrolls when necessary.
No draft is sent, cleared or transferred by a layout change.
