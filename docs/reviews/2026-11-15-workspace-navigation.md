# Workspace keyboard navigation evidence

P7/F09/F16 now provides a keyboard-visible bypass link, destination heading focus
after deliberate navigation, accurate current-page markers across settings and
workspace pages, and a visible fallback when a dialog's invoking control was
removed. Gmail and device settings previously displayed the selected chat title;
their heading now identifies the opened page. Initial loading, title updates,
streaming and implicit thread creation do not request navigation focus.

Actual React App checks cover keyboard bypass, Calendar navigation/return with an
unsent draft, unchanged input focus on passive session refresh, mobile drawer
selection, cancelled deletion after its drawer disappears and completed deletion.
The existing App run passed22 checks and exposed two stale invitation fixtures
returning object/empty responses where the current endpoint returns an array.
Only those fixtures were corrected. The two affected invitation checks and two
new interaction checks passed in the focused rerun; successful unrelated checks
were not repeated. React typecheck and production asset build passed.

An isolated actual Chrome profile with synthetic API responses confirmed Tab to
the bypass link, Enter to the heading, a390x844 drawer selecting Calendar with
the native dialog closed and destination focus retained, return to chat, and a
visible bypass link at1280x800. The rendered screenshot was inspected. This is
browser keyboard/viewport evidence, not a screen-reader, physical-device or full
accessibility conformance claim. No backend, account authority, model, dependency
or CI behavior changes.
