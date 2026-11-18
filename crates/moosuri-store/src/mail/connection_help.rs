//! A model's structured current-request intent can request setup UI, never IO.
use super::*;

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MailConnectionHelpIntent {
    pub current_request: String,
}

impl Store {
    /// Only the runtime calls this after validating one exclusive function call.
    /// No model-selected recovery, URL, owner, secret or answer is accepted.
    pub async fn finish_model_mail_connection_help(
        &self,
        step: &crate::StepLease,
        intent: MailConnectionHelpIntent,
        app: Option<GoogleAppPin>,
    ) -> Result<()> {
        let step = step.clone();
        self.call(move |db| {
            let tx = db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let run = crate::agent_execution::guard_step(&tx, &db.provider_key, &step)?;
            if intent.current_request != run.text
                || crate::missions::is_mission(&tx, &run.tenant, &run.id)?
                || crate::media::has_context(&tx, &run.tenant, &run.id)?
            {
                return Err(StoreError::Forbidden);
            }
            let prior_calls: i64 = tx.query_row(
                "SELECT tool_calls FROM agent_execution WHERE tenant_id=?1 AND run_id=?2",
                params![run.tenant, run.id], |r| r.get(0),
            )?;
            if prior_calls != 0 { return Err(StoreError::Forbidden); }
            // Recheck the current owner and live connections in the same write
            // transaction as the receipt. A newly connected account invalidates
            // an earlier model snapshot rather than minting stale setup UI.
            let context = model_context::connection_context(&tx, &run)?;
            if context.server_read_connected || context.native_read_admitted
                || crate::native_task_mail::current_model_read_connected(&tx, &run)? {
                return Err(StoreError::Conflict);
            }
            if let Some(pin) = &app { google_setup::guard_app(&tx, pin)?; }
            else if tx.query_row("SELECT EXISTS(SELECT 1 FROM google_setup)", [], |r| r.get::<_, bool>(0))? {
                return Err(StoreError::Conflict);
            }
            let (recovery, text) = if app.is_some() && context.needs_reconnect {
                (MailConnectionRecovery::Reconnect, "Gmail 읽기 연결을 다시 인증해야 해요. Gmail 설정에서 본인 계정을 다시 연결한 뒤 여기서 요청해 주세요. 아직 메일은 읽지 않았어요.")
            } else if context.admin {
                (MailConnectionRecovery::GwsSetup, "Gmail을 확인하려면 Gmail 설정에서 본인 기기의 GWS 받은편지함 도우미를 연결해 주세요. GWS 인증은 별도 기기에 유지되며 볼트 연결은 필요하지 않아요. 웹 OAuth를 원하면 관리자인 본인이 Google 앱을 설정하는 기존 방식도 선택할 수 있어요. 아직 메일은 읽지 않았어요.")
            } else {
                (MailConnectionRecovery::GwsSetup, "Gmail을 확인하려면 Gmail 설정에서 본인 기기의 GWS 받은편지함 도우미를 연결해 주세요. GWS 인증은 별도 기기에 유지되며 볼트 연결은 필요하지 않아요. 웹 OAuth를 원하면 설치 관리자에게 Google 앱 준비를 요청하는 기존 방식도 선택할 수 있어요. 아직 메일은 읽지 않았어요.")
            };
            let item = format!("mail-recovery-{}", json(&recovery)?.trim_matches('"'));
            crate::chat::apply_patches(&tx, &run, vec![
                OutputPatch::Append { item, text: String::new(), phase: None, refusal: false },
                OutputPatch::Append { item: "credential-google-preview-result".into(), text: text.into(), phase: Some(MessagePhase::FinalAnswer), refusal: false },
            ])?;
            crate::chat::transition(&tx, &run, RunState::Completed, None)?;
            tx.commit()?;
            Ok(())
        }).await
    }
}
