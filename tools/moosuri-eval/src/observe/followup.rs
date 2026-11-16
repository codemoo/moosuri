//! Read-only persisted monitor/policy/child/notice observations; no model input.
use super::*;
use serde_json::json;
pub fn snapshot(root: &Path, id: &str) -> EvalResult<Value> {
    let mut conn = open(root)?;
    let tx = conn
        .transaction()
        .map_err(|_| "followup_observer_transaction")?;
    let (mut monitor,scope):(Value,String)=tx.query_row("SELECT tenant_id,account_id,id,parent_mission_id,revision,state,sources,policy_id,schedule_id FROM task_monitors WHERE id=?1",[id],|r|Ok((json!({"tenant":r.get::<_,String>(0)?,"account":r.get::<_,String>(1)?,"id":r.get::<_,String>(2)?,"parent":r.get::<_,String>(3)?,"revision":r.get::<_,i64>(4)?,"state":r.get::<_,String>(5)?,"policy_id":r.get::<_,String>(7)?,"schedule_id":r.get::<_,String>(8)?}),r.get(6)?))).map_err(|_|"followup_observer_monitor")?;
    monitor["scope"] = serde_json::from_str(&scope).map_err(|_| "followup_observer_scope")?;
    let schedule = monitor["schedule_id"]
        .as_str()
        .ok_or("followup_observer_schedule")?;
    let policy=tx.query_row("SELECT p.id,p.state,p.cadence_seconds,p.expires_at,p.sources,s.state,s.revision,p.account_id,p.tenant_id FROM task_monitor_policies p JOIN reminder_schedules s ON s.tenant_id=p.tenant_id AND s.id=?2 WHERE p.id=?1",params![monitor["policy_id"].as_str(),schedule],|r|Ok(json!({"id":r.get::<_,String>(0)?,"state":r.get::<_,String>(1)?,"cadence_seconds":r.get::<_,i64>(2)?,"expires_at":r.get::<_,i64>(3)?,"scope":r.get::<_,String>(4)?.parse::<Value>().ok(),"schedule_state":r.get::<_,String>(5)?,"schedule_revision":r.get::<_,i64>(6)?,"account":r.get::<_,String>(7)?,"tenant":r.get::<_,String>(8)?}))).map_err(|_|"followup_observer_policy")?;
    let count: i64 = tx
        .query_row(
            "SELECT count(*) FROM task_monitor_policies WHERE account_id=?1",
            [monitor["account"].as_str()],
            |r| r.get(0),
        )
        .map_err(|_| "followup_observer_policy_count")?;
    let children=tx.prepare("SELECT o.ordinal,o.mission_id,m.run_id,m.state,x.publication_state,x.notification_id,x.schedule_id,x.occurrence_id FROM task_monitor_observations o JOIN missions m ON m.tenant_id=o.tenant_id AND m.id=o.mission_id JOIN recurring_mission_occurrences x ON x.tenant_id=o.tenant_id AND x.mission_id=m.id WHERE o.monitor_id=?1 ORDER BY o.ordinal").map_err(|_|"followup_observer_children")?.query_map([id],|r|Ok(json!({"ordinal":r.get::<_,i64>(0)?,"id":r.get::<_,String>(1)?,"run":r.get::<_,String>(2)?,"state":r.get::<_,String>(3)?,"publication_state":r.get::<_,String>(4)?,"notification_id":r.get::<_,Option<String>>(5)?,"schedule_id":r.get::<_,String>(6)?,"occurrence_id":r.get::<_,String>(7)?}))).map_err(|_|"followup_observer_children")?.collect::<Result<Vec<_>,_>>().map_err(|_|"followup_observer_children")?;
    let notifications=tx.prepare("SELECT n.id,n.account_id,n.dismissed,n.push_suppressed,n.source_suppressed,n.schedule_id,n.occurrence_id,o.mission_id FROM local_notifications n LEFT JOIN recurring_mission_occurrences o ON o.tenant_id=n.tenant_id AND o.schedule_id=n.schedule_id AND o.occurrence_id=n.occurrence_id AND o.notification_id=n.id WHERE n.schedule_id=?1 ORDER BY n.id").map_err(|_|"followup_observer_notices")?.query_map([schedule],|r|Ok(json!({"id":r.get::<_,String>(0)?,"account":r.get::<_,String>(1)?,"dismissed":r.get::<_,bool>(2)?,"push_suppressed":r.get::<_,bool>(3)?,"source_suppressed":r.get::<_,bool>(4)?,"schedule_id":r.get::<_,String>(5)?,"occurrence_id":r.get::<_,String>(6)?,"mission_id":r.get::<_,Option<String>>(7)?}))).map_err(|_|"followup_observer_notices")?.collect::<Result<Vec<_>,_>>().map_err(|_|"followup_observer_notices")?;
    let unchanged:i64=tx.query_row("SELECT count(*) FROM reminder_occurrences WHERE schedule_id=?1 AND state='skipped' AND reason='monitor_no_new_evidence'",[schedule],|r|r.get(0)).map_err(|_|"followup_observer_unchanged")?;
    Ok(
        json!({"monitor":monitor,"policy":policy,"policy_count":count,"children":children,"notifications":notifications,"unchanged_occurrences":unchanged}),
    )
}
