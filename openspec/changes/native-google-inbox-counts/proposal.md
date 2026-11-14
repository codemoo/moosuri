# Trusted-native Google INBOX counts

## Why
P8/F69 needs useful automatic reuse of an explicitly selected user-vault OAuth refresh credential, while keeping secrets and account matching on the trusted device.

## What Changes
Add one typed Google INBOX-count operation and strict numeric projection through the existing signed native dispatch/journal and original-chat waiting machinery. Add personal settings, a separate admission flag and exact local account/scope/origin grant. Preserve existing signed GitHub bytes and resource records.

## Impact
Client broker/protocol, existing Store/agent/server credential path and React settings/status only. Default-off feature stays off; no DB migration, dependency, cache, CI, server credential transfer, Google login/send/write or production action. [Contract/evidence](../../../docs/native-credential-google.md).
