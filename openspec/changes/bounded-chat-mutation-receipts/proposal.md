## Why
A message admission, new-thread creation or run cancel/retry can leave the main chat locked forever when HTTP headers or its JSON body stop arriving. Durable SSE admission can already prove success while that HTTP receipt remains stuck. Retrying an unkeyed creation after a lost receipt can also create an extra empty conversation.

## What Changes
Explicitly bound foreground JSON mutations to one 30-second complete-response deadline. Treat expiration as unknown receipt, preserve the original pending message identity/payload, and reconcile by authenticated GET without automatic mutation replay. Release a pending message transport as soon as canonical history/SSE proves admission. Add an optional thread creation UUID with immutable original-title matching and a retained deletion receipt. Keep the exact creation and message identities across explicit recovery and bounded authenticated tab-draft reload.

## Impact
React main-chat recovery and an additive server/store creation receipt contract at schema71. Existing clients without a creation request_id retain their behavior. No provider, model/effort, account permission or final mail approval changes. Other mutation/probe/upload deadlines remain unchanged.
