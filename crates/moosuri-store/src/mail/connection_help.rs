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
            } else if app.is_some() {
                (MailConnectionRecovery::Connect, "Gmail 설정에서 본인 Google 계정으로 로그인하고 읽기 권한을 허용해 주세요. 연결은 이 서버에 보관되며 기기 도우미는 필요하지 않아요. 아직 메일은 읽지 않았어요.")
            } else if context.admin {
                (MailConnectionRecovery::AdminSetup, "이 서버의 Google OAuth 앱이 아직 준비되지 않았어요. 관리자인 본인이 /admin/google에서 앱을 설정한 뒤 Gmail 설정에서 본인 Google 계정의 읽기 권한을 연결해 주세요. 아직 메일은 읽지 않았어요.")
            } else {
                (MailConnectionRecovery::MemberSetup, "이 서버의 Google OAuth 앱이 아직 준비되지 않았어요. 설치 관리자에게 앱 설정을 요청한 뒤 Gmail 설정에서 본인 Google 계정의 읽기 권한을 연결해 주세요. 아직 메일은 읽지 않았어요.")
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
