# Design

`Pending` derives answer availability from final per-item metadata, including
metadata received after text. Nonempty commentary remains visible but cannot
satisfy successful answer completion. Providers without phase metadata retain
their existing unphased answer support.

Retry admission distinguishes local finalization, consuming a completed native
tool result with a model round, and ordinary model-and-tool recovery. Only a
completed result for the current remote step qualifies for the second case.
Remote dispatch and tool commit continue enforcing their own ceilings; counters,
deadline, owner authority and clock/revision fences are not reset.
