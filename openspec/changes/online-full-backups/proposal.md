# Online full-data encrypted snapshots

P1/P9/F52 currently require stopping the service to export full data. Add an
optional operator-configured periodic worker using the live single writer and
the existing format2 export/restore contract. Keep independent journal latestness
separate. No schema, dependency, user approval or model-call changes.
