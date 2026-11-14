The only production model writes are initial chat/missions insertion and this new
module's full-Value read-modify-write. StoredRunChoice parses but does not rewrite
existing JSON. A private run_observations_v1 extension therefore preserves numeric
attempt observations without schema0048. Original model/effort/profile and all
other extension members remain untouched. No old timestamp backfill is performed.

Admission/claim use one process monotonic clock. Queue ends before context assembly;
context measures only server chat-history assembly. First dispatch is the committed
model-call intent, not socket/provider acceptance. Runtime measures its model-stream
interval, including transport/server backpressure; tool-handler intervals include
local Store/cache work and external waits/authority cleanup. Started spans whose end
is not observed remain missing, with measured-call denominators. No writer is held
while awaiting IO. Timing writes do not authorize execution and do not reset budgets.

Body arrival marks are captured before buffering, attached to existing guarded
output persistence, and stored by local numeric output ordinal. No provider item ID,
prompt, text, tool name/arguments, credential, model label or URL is exported.
Only surviving explicit final_answer, nonrefusal, nonwhitespace output on a completed
attempt qualifies. Commentary/tool-only/absent phase/error/partial output never
becomes a final-body sample. A later phase resolution can qualify the original body
arrival. Repeated body patches do not rewrite metrics after the initial mark.

Each explicit retry retains its original attempt number and prior numeric records.
Missing historical attempts use unobserved outcome counts, never guessed outcomes.
Direct SQL terminal paths have their current outcome counted but no invented terminal
time. Reopening active runs marks cross-open spans discontinuous; previously measured
local durations remain observations. Monotonic measurements do not subtract wall
clocks, and browser/input/network-roundtrip/DOM latency is not inferred.

Owner/admin existing usage responses gain fixed-key outcome counts and nearest-rank
p50/p95 with sample/missing denominators over the latest128 retained runs (at most32
attempts each). No percentile is returned without a sample. Existing token accounting
continues across deleted content; this timing window is explicitly retained history,
so deleting a conversation removes its timing JSON as well. No all-time timing or
actual-provider/production-load benchmark is claimed. General historical GC remains
outside this unit.
