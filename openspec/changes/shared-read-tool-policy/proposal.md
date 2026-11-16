# Shared built-in read-tool policy

Model tool exposure and durable admission currently repeat tool-name/variant lists.
A new read tool can leak into a mission through its negative filter while Store
rejects it. Live guidance also lacks an explicit durable tool-class fence.

Use one closed static classification in moosuri-core for the existing live-guidance,
mission and task-monitor upper bounds. Keep current contextual account/profile,
recipe, exact-source, effect and lifecycle checks. This does not add tools, grants,
a plugin registry or user approval steps; mail send, Calendar effects, native
credentials, vault and report permissions remain unchanged.
