# Reuse validated model connections

## Why
Every model tool round rebuilt the HTTP client and disabled idle pooling. Repeated
handshakes add latency even when the same configured client and origin remain valid.

## What Changes
- Retain a single bounded idle connection per configured client after fresh DNS validation.
- Retire its pool when origin/address set or transport limits change.
- Preserve no retries, redirect/proxy denial, deadlines, clean EOF and owner-scoped client lifetime.

## Impact
F03/F26 model transport implementation and actual TCP/protocol regression checks.
No model/effort changes, new service, schema or deployment.
