# Change: three bounded external-task recipes

## Why
Users delegate public research, inbox triage and reply preparation in ordinary
language. Existing real tools can perform these tasks without a new classifier or
authority surface. This applies R16/R20/R23/R24/R25 and F24/F25/F27/F34/F35/F42 within the
currently implemented boundaries; it does not claim every planned feature is ready.

## What changes
Append original `web-research`, `inbox-briefing` and `mail-reply` instructions to the
bundled metadata, with proposed sidecars, licenses and synthetic examples. Preserve
the original three recipes and all admitted pins. Document missing capabilities,
coverage, calculation/date checks, the shared budget and final approval for EVERY send.

## Impact
Only recipe registration/content and scoped documentation. No tools, schemas,
credentials, OAuth, provider calls, browser actions, approval APIs or send authority.
The reply recipe creates a local reply-style draft; current chat tools do not
promise original Gmail-thread placement or attachments. See docs/external-task-recipes.md.
