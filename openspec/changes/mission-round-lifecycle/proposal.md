# Consistent committed mission rounds

Fresh and reopened missions currently duplicate guidance, plan, tool, clarification
and final handling. A typed blank final or empty legacy plan can fail Store
validation and leave the run active. Reopened malformed guided output can also
lose its assigned-guidance handling and spend another model call.

Use one mission-specific committed-round handler and one guarded exact-step read
for received guidance. Keep model terminal admission, original budgets, acceptance
and owner/source/profile authority, domain effects and verification unchanged.
No automatic approval, new effect, schema, DTO or general runner framework is added.
