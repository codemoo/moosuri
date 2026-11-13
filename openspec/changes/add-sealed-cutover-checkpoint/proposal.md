# Why

A manual export cannot prove later private withdrawals are enforced after host
loss. Source/derived-state updates are broader than direct deletion journal heads.
The smallest next unit is a current-state snapshot whose source can acknowledge
no subsequent work, with an independently carried exact head.

# What Changes

- Permanently seal a stopped initialized configured installation through a durable
  monotonic cutover row and common runtime/setup/recovery admission.
- Capture its full sealed DB and three exact complete/applied journals in a distinct
  six-entry standard age checkpoint, preserving ordinary export codecs.
- Fix IDs and ready ciphertext before durable no-overwrite ciphertext/head output;
  retry sync/publication failures without unsealing or changing adopted bytes.
- Expose local status and document the separately trusted manual head handoff.

# Impact

Integrated schema19 after artifacts16/schedules17/accounts18; matching restore is20.
No new dependencies, restore/import, periodic/off-host latestness or activation.
Root can hold exposing this irreversible CLI until restore C is usable.
