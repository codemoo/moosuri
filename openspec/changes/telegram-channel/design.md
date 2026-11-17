# Design

Rust Bot API connector fixes the production origin, bounds responses, redacts
errors and splits plain text conservatively below4096 UTF-16 units. No SDK is
needed. Long polling and sending are separate supervised children; no extra
model process, instruction stack or scheduler is introduced.

Schema75 stores encrypted installation credentials, one-use hashed pairings,
independent session-bound user/chat mappings, atomic update checkpoints and a
bounded durable outbox. Existing Store transactions admit turns and live guidance.
Final send admission revalidates current account/binding/source/notice state.
Dispatch commitment precedes network IO; uncertain sends never retry. Explicit
flood control can retry up to five times with its required delay. Restore/export
clear connections so recovered state cannot start another poller or resend old work.

React admin settings verify the bot and reject existing webhooks; personal settings
mint a temporary deep link and optionally enable generic inbox links. Every API
uses existing authenticated account and Origin middleware. Tokens and pairing
links live only in memory in the UI. Existing arbitrary Telegram chats/groups,
attachments and Telegram effect-approval buttons are outside this first scope.
