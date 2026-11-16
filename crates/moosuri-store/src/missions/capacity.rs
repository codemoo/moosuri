//! Shared mission slot policy. Callers retain authority and transactional ordering.
use super::*;

/// New work consumes a global place as well as its initial owner slot.
pub(crate) fn new_mission_available(
    c: &Connection,
    tenant: &str,
    account: &str,
    waiting: bool,
) -> Result<bool> {
    let global: i64 = c.query_row(
        "SELECT count(*) FROM missions WHERE state IN('queued','planning','executing','verifying','paused','waiting')",
        [],
        |r| r.get(0),
    )?;
    let owner = if waiting {
        waiting_available(c, tenant, account)?
    } else {
        runnable_available(c, tenant, account, None)?
    };
    Ok(global < 16 && owner)
}

/// Existing work changes slots; it must not acquire another global place.
pub(crate) fn waiting_available(c: &Connection, tenant: &str, account: &str) -> Result<bool> {
    let waiting: i64 = c.query_row(
        "SELECT count(*) FROM missions WHERE tenant_id=?1 AND account_id=?2 AND state='waiting'",
        params![tenant, account],
        |r| r.get(0),
    )?;
    Ok(waiting < 2)
}

/// Paused work still occupies its owner's runnable slot. Resume excludes only
/// the current mission; new admission passes None and excludes nothing.
pub(crate) fn runnable_available(
    c: &Connection,
    tenant: &str,
    account: &str,
    except: Option<&str>,
) -> Result<bool> {
    Ok(!c.query_row(
        "SELECT EXISTS(SELECT 1 FROM missions WHERE tenant_id=?1 AND account_id=?2 AND (?3 IS NULL OR id!=?3) AND state IN('queued','planning','executing','verifying','paused'))",
        params![tenant, account, except],
        |r| r.get::<_, bool>(0),
    )?)
}
