# Design

Store owns collection grants, exact account epoch/model/source pins, opaque jobs,
byte/output reservations, fact dedup and immutable extraction origin. A separate
Rust worker consumes only complete source paragraphs and an index-only model
result. Subject/speaker retain a reported label; no sender becomes verified self.
Normal ingestion revision is separate from grant revision. Reported-author mode
supports many senders per collection; specified mode handles owner-attributed
manual/unlabeled text and rejects a mismatched existing speaker.

Jobs store no prompt, selected paragraph or continuation text. Source withdrawal
cancels leases; knowledge generation pins prevent suppression changes from
reindexing an acquired namespace. Exact source quote suppression is filtered before
dispatch and checked at atomic commit. All selected writes and terminal receipt
commit together. Lost final acknowledgement returns the same receipt; interrupted
remote work is never automatically redispatched. Logout leaves the independent
grant intact, password/recovery and current provider/source policy fence it.

One job has one call,120 seconds,8KiB of complete paragraphs,16 units and4 facts.
Daily defaults10 calls/24,000 serialized request bytes/4,000 reserved output tokens;
1024 output tokens are reserved per dispatch with the protocol output cap.
Byte counting includes public OpenRouter routing metadata, not HTTP/TLS framing.
No tokenizer or pricing dependency is introduced. Observed usage remains separate.

Schema integration: isolated migration0014 applies after12 as local version13;
parent must apply backup0013 first, then use final version14 and reconcile schema
assertions/preflight. No fake0013 is included. Semantic attribution and no-fact
quality require a separately frozen live evaluation; scripted decisions only prove
protocol/authority/lifetime behavior.
