# Recover bounded browser login information

P6/F web browser workspaces currently lose login information after process restart.
An operator may opt into dedicated server-managed Docker profiles, retained only
within the original workspace lifetime. A person explicitly recovers a blank
browser and inspects prior results without replaying navigation or input.

The existing one-shot public reader and deployments without this opt-in retain
profile discard. This does not add automatic authenticated work, long-term
login retention, vault security, personal host profiles, or external-action replay.
