use super::planning::{Admission, PlanningSession};
use super::*;
pub(super) async fn run(
    session: &PlanningSession<'_>,
    context: AgentContext,
    text: &str,
    calendar: &crate::calendar::CalendarService,
) -> Option<AgentContext> {
    let planning = match session.store.mission_chat_planning(session.lease).await {
        Ok(Some(v)) => v,
        Ok(None) => return Some(context),
        Err(e) => {
            session.failed(None, e).await;
            return None;
        }
    };
    let candidate = if planning.continuation.is_some()
        || planning.receipt_target_available && crate::calendar_intent::followup_candidate(text)
    {
        true
    } else if planning.calendar_only {
        crate::calendar_intent::candidate(text)
    } else {
        crate::mission_intent::candidate(text, planning.selected_kind.is_some())
    };
    if !candidate {
        if let Err(error) = session.store.skip_mission_chat(session.lease).await {
            session.failed(None, error).await;
            return None;
        }
        return Some(context);
    }
    let text = planning.request_text.as_str();
    let metadata = serde_json::to_string(&planning).ok()?;
    let request = if planning.calendar_only {
        crate::calendar_intent::personal_request(
            session.request.model.clone(),
            session.request.effort.clone(),
            text,
            &metadata,
        )
    } else {
        crate::mission_intent::request(
            session.request.model.clone(),
            session.request.effort.clone(),
            text,
            &metadata,
        )
    };
    let definitions = if planning.calendar_only {
        crate::calendar_intent::personal_definitions()
    } else {
        crate::mission_intent::definitions()
    };
    let (_, next, _) = planning_round(session, &context, &request, &definitions).await?;
    match calendar.chat(session.lease).await {
        Ok(true) => {
            session.signals.changed();
            return None;
        }
        Ok(false) => {}
        Err(e) => {
            session.failed(None, e).await;
            return None;
        }
    }
    let _gate = session.signals.delivery.write().await;
    match session.store.apply_mission_chat(session.lease).await {
        Ok(Some(false)) => Some(next),
        Ok(Some(true)) => {
            session.signals.changed();
            session.signals.wake.notify_one();
            None
        }
        Ok(None) => {
            session.failed(None, StoreError::Conflict).await;
            None
        }
        Err(e) => {
            session.failed(None, e).await;
            None
        }
    }
}
async fn planning_round(
    session: &PlanningSession<'_>,
    context: &AgentContext,
    request: &TextRequest,
    definitions: &[moosuri_connectors::model::protocol::tools::FunctionDefinition],
) -> Option<(ToolRound, AgentContext, StepLease)> {
    let (round, step) = session
        .dispatch(context, request, definitions, Admission::MissionChat)
        .await?;
    let Some(decision) = crate::mission_intent::parse(&round) else {
        finish(
            session.store,
            session.lease,
            Some(&step),
            RunState::Failed,
            Some(RunFailure::ProtocolFailed),
            session.signals,
        )
        .await;
        return None;
    };
    let bytes = serde_json::json!({"kind":"mission_chat_decision","decision":decision})
        .to_string()
        .into_bytes();
    let next = session.commit(&step, bytes).await?;
    Some((round, next, step))
}
