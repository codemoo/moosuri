# Settle guidance and foreground answers atomically

## Why

Foreground completion acknowledges assigned guidance, attempts terminal completion
and demotes a superseded answer in separate transactions. A later write failure
can leave guidance marked applied or authorize another round while the old answer
still appears final.

## What Changes

Commit guidance acknowledgment and the answer's completed/continue disposition
in one guarded transaction. Consolidate execution settlement in one Store module,
reuse the existing completion rules and remove the standalone supersede writer.
Apply the existing Busy-only retry policy to local instruction acknowledgment and
answer settlement; surface exhausted/permanent failures through existing outcomes.

## Impact

Foreground answer/guidance persistence and shared execution settlement. Mission
verification stays separate. No HTTP API, schema, model setting or confirmation
change. Automatic CI remains off.
