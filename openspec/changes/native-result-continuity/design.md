# Design

The native-resume path already reissues the original completed step lease and reuses saved typed results. Result projection must use that actual step and call ID, exactly as the ordinary foreground path does. A single serializer adds direct receipt addresses only to Search/Source results under an evidence-calculation profile; all other result shapes remain unchanged. Existing `ToolResult.is_error` handling remains at the callers.

The regression exercises local-first already-saved results and native-first deferred local results through actual signed synthetic broker settlement. It consumes the returned search/read addresses in a real evidence calculation, checks the exact total and ensures the final continuation contains one search/read with no model polling while waiting. Provider calls, budget admission, cancellation and grant checks are unchanged.
