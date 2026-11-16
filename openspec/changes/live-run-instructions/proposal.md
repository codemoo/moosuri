# Add guidance while the same assistant task runs

Owners currently cannot add a constraint while an answer or mission is running
without stopping it or starting another task. Add an immediate durable receipt
and apply explicit current-owner guidance at the next safe inference boundary.
Preserve the original run/mission, completed tools, dispatch identities and budget.
Expose actual received/applied/closed receipts, exact uncertain-ACK lookup and
explicit support flags. Initial support is confined to safe read-only function
continuations and read-only missions; typed local actions, native credentials,
Mail/Calendar effects, private-report/artifact-write stages and plain text-only transports remain
unsupported. No model call is needed to acknowledge a request.
