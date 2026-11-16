//! Read-only mission input, original-work and publication boundary observations.
use super::*;
use serde_json::json;
pub fn snapshot(root: &Path, id: &str) -> EvalResult<Value> {
    let mut conn = open(root)?;
    let tx = conn
        .transaction()
        .map_err(|_| "repair_choice_observer_transaction")?;
    let (mut mission, payload, candidate, run, profile):(Value,String,Option<String>,String,String)=tx.query_row("SELECT m.tenant_id,m.account_id,m.run_id,m.state,m.revision,m.grant_enabled,m.uncertain,m.payload,m.candidate,m.expires_ms,m.repairs,r.model FROM missions m JOIN chat_runs r ON r.tenant_id=m.tenant_id AND r.id=m.run_id WHERE m.id=?1",[id],|r|Ok((json!({"tenant":r.get::<_,String>(0)?,"account":r.get::<_,String>(1)?,"id":id,"run":r.get::<_,String>(2)?,"state":r.get::<_,String>(3)?,"revision":r.get::<_,i64>(4)?,"grant":r.get::<_,bool>(5)?,"uncertain":r.get::<_,bool>(6)?,"expires_ms":r.get::<_,i64>(9)?,"repairs":r.get::<_,i64>(10)?}),r.get(7)?,r.get(8)?,r.get(2)?,r.get(11)?))).map_err(|_|"repair_choice_observer_mission")?;
    mission["request"] =
        serde_json::from_str(&payload).map_err(|_| "repair_choice_observer_payload")?;
    mission["candidate"] = candidate
        .map(|s| serde_json::from_str::<Value>(&s))
        .transpose()
        .map_err(|_| "repair_choice_observer_candidate")?
        .unwrap_or(Value::Null);
    mission["model_profile_sha256"] = json!(crate::digest(profile.as_bytes()));
    let execution=tx.query_row("SELECT remote_calls,tool_calls,deadline_ms FROM agent_execution WHERE run_id=?1",[&run],|r|Ok(json!({"remote_calls":r.get::<_,i64>(0)?,"tool_calls":r.get::<_,i64>(1)?,"deadline_ms":r.get::<_,i64>(2)?}))).map_err(|_|"repair_choice_observer_execution")?;
    let waits=tx.prepare("SELECT request_id,field,status,value,acceptance_revision,expires_ms,candidates,notification_id FROM mission_input_waits WHERE mission_id=?1 ORDER BY step").map_err(|_|"repair_choice_observer_waits")?.query_map([id],|r|Ok(json!({"request_id":r.get::<_,String>(0)?,"field":r.get::<_,String>(1)?,"state":r.get::<_,String>(2)?,"value":r.get::<_,Option<String>>(3)?,"acceptance_revision":r.get::<_,i64>(4)?,"expires_ms":r.get::<_,i64>(5)?,"candidates":r.get::<_,String>(6)?.parse::<Value>().ok(),"notification_id":r.get::<_,String>(7)?}))).map_err(|_|"repair_choice_observer_waits")?.collect::<Result<Vec<_>,_>>().map_err(|_|"repair_choice_observer_waits")?;
    let replies=tx.prepare("SELECT request_id,payload FROM mission_input_replies WHERE mission_id=?1 ORDER BY request_id").map_err(|_|"repair_choice_observer_replies")?.query_map([id],|r|Ok(json!({"request_id":r.get::<_,String>(0)?,"payload":r.get::<_,String>(1)?.parse::<Value>().ok()}))).map_err(|_|"repair_choice_observer_replies")?.collect::<Result<Vec<_>,_>>().map_err(|_|"repair_choice_observer_replies")?;
    let notices=tx.prepare("SELECT n.id,n.dismissed,n.push_suppressed FROM local_notifications n JOIN mission_input_waits w ON w.tenant_id=n.tenant_id AND w.notification_id=n.id WHERE w.mission_id=?1 ORDER BY n.id").map_err(|_|"repair_choice_observer_notices")?.query_map([id],|r|Ok(json!({"id":r.get::<_,String>(0)?,"dismissed":r.get::<_,bool>(1)?,"push_suppressed":r.get::<_,bool>(2)?}))).map_err(|_|"repair_choice_observer_notices")?.collect::<Result<Vec<_>,_>>().map_err(|_|"repair_choice_observer_notices")?;
    let (controls,artifacts,mail,calendar,output):(i64,i64,i64,i64,i64)=tx.query_row("SELECT (SELECT count(*) FROM mission_controls WHERE mission_id=?1),(SELECT count(*) FROM personal_artifacts),(SELECT count(*) FROM mail_actions),(SELECT count(*) FROM calendar_actions),(SELECT count(*) FROM chat_output WHERE run_id=?2)",params![id,run],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).map_err(|_|"repair_choice_observer_effects")?;
    Ok(
        json!({"mission":mission,"execution":execution,"waits":waits,"replies":replies,"notices":notices,"controls":controls,"effects":{"artifacts":artifacts,"mail":mail,"calendar":calendar,"chat_output":output}}),
    )
}
