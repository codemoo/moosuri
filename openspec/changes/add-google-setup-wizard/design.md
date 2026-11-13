# Design

The setup wizard belongs at `/admin/google`. Its three stages are Google Cloud
preparation, local file validation/save, and the administrator's own personal OAuth
connection/profile check. The exact callback comes from ServerConfig's configured
origin, never a request Host/forwarded header or uploaded provider endpoint. The
server uses fixed Google endpoints and keeps imported JSON out of responses/logs.

One installation row stores app identity, callback, monotonically increasing revision
and an encrypted secret. Admin reads and CAS writes validate current role inside the
SQLite writer. A private app carrier resolves configuration for each operation.
An explicit `--google-client-file` stays authoritative for that process; the wizard
shows operator-file mode and refuses web saves rather than hiding an override.

Every pending OAuth row pins the selected app and the original personal session.
All app edits wait until unexpired OAuth exchanges finish or are cancelled. A new
client identity/callback cannot take over existing incompatible personal grants.
Same-client secret rotation checks the app revision on refresh success and failure;
running Gmail sync leases are fenced so stale results cannot mark the new setup as
failed. Persisted local sources/checkpoints are not removed by client-key rotation.

A setup-start flag stored with OAuth state controls return to the wizard. It never
accepts an arbitrary return URL. Consent completion uses the normal personal token
exchange and Gmail profile query. An additional connection check can only access the
requesting administrator's personal connection and reads no message body or sends.

JSON file validation proves format and callback agreement only. Google Cloud API
activation, consent audience, test users, publishing/verification and revocation can
only be assessed by actual authorization/provider behavior. Document the Google
Testing refresh-token lifetime without claiming local validation checks it.
