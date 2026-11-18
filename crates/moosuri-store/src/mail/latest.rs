//! Provider-independent, current-owner literal latest-INBOX reads.
use super::*;
use crate::chat::{self, RunLease};

// Whole-message grammar: source text, quotes, negations, other owners and
// compound actions cannot obtain mail authority by mentioning a matching phrase.
fn intent(text: &str) -> bool {
    if text.len() > 512 || text.contains(['\n', '\r']) {
        return false;
    }
    let text = text
        .trim()
        .trim_end_matches(['?', '!', '.', '？'])
        .to_lowercase();
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let text = ["아니 ", "그러니까 ", "아니요 ", "i mean ", "i asked "]
        .iter()
        .find_map(|prefix| text.strip_prefix(prefix))
        .unwrap_or(&text);
    let en = text.strip_prefix("please ").unwrap_or(text);
    let en = en.strip_suffix(" please").unwrap_or(en);
    if [
        "what is my latest email",
        "what's my latest email",
        "what is my most recent email",
        "what was the last email i received",
        "show my latest email",
        "show me my latest email",
        "read my latest email",
        "show my latest inbox email",
        "what is the latest email in my inbox",
        "show me the latest email in my inbox",
        "what is my latest received email",
        "what was my latest email",
        "what email did i receive last",
        "what's the latest email i received",
        "what is the latest email i received",
        "tell me what my latest email is",
        "what's my most recent email",
        "show me the last email i received",
        "show me the most recent email i received",
        "what's the most recent email in my inbox",
        "tell me my latest email",
    ]
    .contains(&en)
    {
        return true;
    }
    let ko = ["내 ", "나의 ", "내가 "]
        .iter()
        .find_map(|p| text.strip_prefix(p))
        .unwrap_or(&text);
    let mails = ["gmail 메일", "이메일", "지메일", "메일", "gmail"];
    let recencies = [
        "가장 최근에 받은 ",
        "제일 최근에 받은 ",
        "최근에 받은 ",
        "마지막으로 받은 ",
        "가장 최근에 온 ",
        "제일 최근에 온 ",
        "최근에 온 ",
        "마지막으로 온 ",
        "최근 온 ",
        "최근 받은 ",
        "최근에 도착한 ",
        "최근 수신한 ",
        "최신 ",
    ];
    let actions = [
        " 알려줘",
        " 알려 줘",
        " 알려주세요",
        " 알려 주세요",
        " 보여줘",
        " 보여 줘",
        " 보여주세요",
        " 보여 주세요",
        " 확인해줘",
        " 확인해 줘",
        " 읽어줘",
        " 읽어 줘",
        "이 뭐야",
        "은 뭐야",
        " 뭐야",
        "이 뭔지 알려줘",
        "은 무엇인가요",
        "이 무엇인가요",
        "을 알려줘",
        "을 보여줘",
        "을 보여 줘",
        "이 뭐냐고",
        "이 뭐냐고요",
        " 뭐냐고",
        " 뭐냐고요",
        " 알려달라고",
        " 보여달라고",
    ];
    if let Some(rest) = recencies.iter().find_map(|p| ko.strip_prefix(p)) {
        return mails
            .iter()
            .filter_map(|p| rest.strip_prefix(p))
            .any(|suffix| actions.contains(&suffix));
    }
    if let Some(rest) = mails
        .iter()
        .filter_map(|p| ko.strip_prefix(p))
        .find_map(|v| v.strip_prefix(' '))
    {
        let questions = [
            "뭐야",
            "뭐냐고",
            "뭐냐고요",
            "뭐냐구",
            "뭔데",
            "뭐였냐고",
            "알려달라고",
            "보여달라고",
            "뭔지알려달라고",
            "뭐예요",
            "뭐였어",
            "뭔지알려줘",
            "무엇인가요",
            "알려줘",
            "알려주세요",
            "보여줘",
            "보여주세요",
            "확인해줘",
        ];
        return recencies
            .iter()
            .filter_map(|p| rest.strip_prefix(p.trim_end()))
            .any(|rest| {
                let rest: String = rest.chars().filter(|c| !c.is_whitespace()).collect();
                ["것은", "것이", "것", "거", "게", "건", ""]
                    .iter()
                    .filter_map(|pronoun| rest.strip_prefix(pronoun))
                    .any(|predicate| questions.contains(&predicate))
            });
    }
    false
}

// This grammar only requests setup help when no usable read connection exists.
// It never authorizes mail IO; pin_latest_inbox still requires intent() above.
fn connection_help_intent(text: &str) -> bool {
    if intent(text) {
        return true;
    }
    if text.len() > 512 || text.contains(['\n', '\r']) {
        return false;
    }
    let text = text
        .trim()
        .trim_end_matches(['?', '!', '.', '？'])
        .to_lowercase();
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let text = ["아니 ", "그러니까 ", "아니요 ", "i mean "]
        .iter()
        .find_map(|prefix| text.strip_prefix(prefix))
        .unwrap_or(&text);
    let en = text.strip_prefix("please ").unwrap_or(text);
    if [
        "check my email",
        "check my emails",
        "read my email",
        "show my inbox",
        "show me my inbox",
        "summarize my inbox",
        "summarise my inbox",
        "summarize my recent emails",
        "show my unread emails",
        "do i have any new email",
        "do i have any new emails",
        "check my gmail inbox",
        "what are my recent emails",
        "show me my recent emails",
        "show me my unread emails",
        "any new emails in my inbox",
    ]
    .contains(&en)
    {
        return true;
    }
    let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    let ko = ["내가", "나의", "내"]
        .iter()
        .find_map(|p| compact.strip_prefix(p))
        .unwrap_or(&compact);
    let ko = ["최근", "새로온", "안읽은", "오늘온", "오늘받은"]
        .iter()
        .find_map(|p| ko.strip_prefix(p))
        .unwrap_or(ko);
    [
        "gmail받은편지함",
        "지메일받은편지함",
        "gmail메일",
        "이메일",
        "받은편지함",
        "지메일",
        "메일",
        "gmail",
    ]
    .iter()
    .filter_map(|p| ko.strip_prefix(p))
    .any(|rest| {
        let rest = rest.strip_prefix(['을', '를']).unwrap_or(rest);
        [
            "확인해줘",
            "확인해줘요",
            "확인해주세요",
            "좀확인해줘",
            "읽어줘",
            "읽어주세요",
            "요약해줘",
            "요약해주세요",
            "보여줘",
            "보여주세요",
            "알려줘",
            "확인해달라고",
        ]
        .contains(&rest)
    })
}

pub(crate) fn recovery_view(
    conn: &Connection,
    tenant: &str,
    run: &str,
) -> Result<Option<MailConnectionRecovery>> {
    let item: Option<String> = conn.query_row("SELECT item FROM chat_output WHERE tenant_id=?1 AND run_id=?2 AND item IN('mail-recovery-admin_setup','mail-recovery-member_setup','mail-recovery-connect','mail-recovery-reconnect','mail-recovery-choose_account') LIMIT 1", params![tenant,run], |r|r.get(0)).optional()?;
    item.map(|item| {
        parse(&format!(
            "\"{}\"",
            item.trim_start_matches("mail-recovery-")
        ))
    })
    .transpose()
}

impl Store {
    /// Cheap discovery only; authority is revalidated against the stored text.
    pub fn latest_inbox_intent(text: &str) -> bool {
        intent(text)
    }

    pub fn mail_connection_help_intent(text: &str) -> bool {
        connection_help_intent(text)
    }

    /// The original stored request, never history or model text, authorizes this
    /// narrow read. No synthetic user or model tool capability is manufactured.
    pub async fn latest_inbox_request(
        &self,
        lease: &RunLease,
    ) -> Result<Option<(UserContext, Vec<MailConnection>, bool)>> {
        let lease = lease.clone();
        self.call(move |db| {
            let tx = db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let run = chat::guard(&tx, &db.provider_key, &lease)?;
            if !connection_help_intent(&run.text) || crate::missions::is_mission(&tx, &run.tenant, &run.id)? { return Ok(None); }
            let user = chat::authority(&tx, &run)?;
            let ids = tx.prepare("SELECT id FROM mail_connections WHERE tenant_id=?1 AND account_id=?2 ORDER BY id LIMIT 17")?.query_map(params![user.tenant.0, user.account.id], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
            let connections = ids.iter().map(|id| connection(&tx, &user, id)).collect::<Result<Vec<_>>>()?;
            tx.commit()?;
            Ok(Some((user, connections, intent(&run.text))))
        }).await
    }

    /// Pin before IO so withdrawal purges this literal result as well as tools.
    pub async fn pin_latest_inbox(&self, lease: &RunLease, id: String, epoch: i64) -> Result<()> {
        let lease = lease.clone();
        self.call(move |db| {
            let tx = db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let run = chat::guard(&tx, &db.provider_key, &lease)?;
            if !intent(&run.text) || crate::missions::is_mission(&tx, &run.tenant, &run.id)? { return Err(StoreError::Forbidden); }
            let user = chat::authority(&tx, &run)?;
            let c = connection(&tx, &user, &id)?;
            let count: i64 = tx.query_row("SELECT count(*) FROM mail_connections WHERE tenant_id=?1 AND account_id=?2 AND status='active' AND native_send=0", params![user.tenant.0, user.account.id], |r| r.get(0))?;
            if count != 1 || c.status != "active" || c.native_send || c.epoch != epoch { return Err(StoreError::Conflict); }
            let scopes: String = tx.query_row("SELECT scopes FROM mail_connections WHERE tenant_id=?1 AND id=?2", params![run.tenant, id], |r| r.get(0))?;
            if !parse::<Vec<String>>(&scopes)?.iter().any(|s| s == moosuri_connectors::gmail::READ_SCOPE) { return Err(StoreError::Forbidden); }
            tx.execute("INSERT OR IGNORE INTO mail_run_dependencies VALUES(?1,?2,?3,?4)", params![run.tenant, run.id, id, epoch])?;
            tx.commit()?;
            Ok(())
        }).await
    }

    /// Atomic output + terminal state, under the original lease and owner fence.
    /// The existing display-only marker excludes mail from later model context,
    /// conversation recall, external report provenance and recipe learning.
    pub async fn finish_latest_inbox(
        &self,
        lease: &RunLease,
        text: String,
        binding: Option<&MailBinding>,
    ) -> Result<()> {
        self.finish_inbox_result(lease, text, binding, None).await
    }
    pub async fn finish_mail_connection_guidance(
        &self,
        lease: &RunLease,
        text: String,
        recovery: MailConnectionRecovery,
    ) -> Result<()> {
        self.finish_inbox_result(lease, text, None, Some(recovery))
            .await
    }
    async fn finish_inbox_result(
        &self,
        lease: &RunLease,
        text: String,
        binding: Option<&MailBinding>,
        recovery: Option<MailConnectionRecovery>,
    ) -> Result<()> {
        let pin = binding.map(|b| (b.id.clone(), b.epoch, b.app.clone()));
        let lease = lease.clone();
        self.call(move |db| {
            let tx = db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let run = chat::guard(&tx, &db.provider_key, &lease)?;
            if !(intent(&run.text) || recovery.is_some() && connection_help_intent(&run.text)) || crate::missions::is_mission(&tx, &run.tenant, &run.id)? { return Err(StoreError::Forbidden); }
            let user = chat::authority(&tx, &run)?;
            if let Some(recovery) = recovery {
                if matches!(recovery, MailConnectionRecovery::AdminSetup) && user.account.role != Role::Admin { return Err(StoreError::Forbidden); }
                let item = format!("mail-recovery-{}", json(&recovery)?.trim_matches('"'));
                chat::apply_patches(&tx, &run, vec![OutputPatch::Append { item, text: String::new(), phase: None, refusal: false }])?;
            }
            if let Some((id, epoch, app)) = &pin {
                let c = connection(&tx, &user, id)?;
                if c.status != "active" || c.native_send || c.epoch != *epoch { return Err(StoreError::Conflict); }
                let count: i64 = tx.query_row("SELECT count(*) FROM mail_connections WHERE tenant_id=?1 AND account_id=?2 AND status='active' AND native_send=0", params![user.tenant.0, user.account.id], |r| r.get(0))?;
                if count != 1 { return Err(StoreError::Conflict); }
                let app = app.as_ref().ok_or(StoreError::Unavailable)?;
                google_setup::guard_mail_app(&tx, &run.tenant, id, app)?;
            }
            chat::apply_patches(&tx, &run, vec![OutputPatch::Append { item: "credential-google-preview-result".into(), text, phase: Some(MessagePhase::FinalAnswer), refusal: false }])?;
            chat::transition(&tx, &run, RunState::Completed, None)?;
            tx.commit()?;
            Ok(())
        }).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn latest_intent_requires_one_whole_current_owner_request() {
        for text in [
            "내가 가장 최근에 받은 이메일이 뭐야?",
            "내 이메일 가장 최근에 온 게 뭐야?",
            "내 이메일 가장 최근에 온게 뭐야?",
            "내 이메일 최근에 온거 뭐야",
            "내 이메일 최근에 온거 뭐냐고",
            "아니 내 이메일 최근에 온 거 뭐냐고요?",
            "그러니까 내 메일 최근에 받은 게 뭔데?",
            "내 최근에 받은 메일이 뭐냐고?",
            "I mean what's my latest email?",
            "What email did I receive last?",
            "내 이메일 최근에 온 거 뭐야?",
            "내 메일 최근에 받은 건 뭐야?",
            "지메일 마지막으로 온 것 알려 줘",
            "가장 최근에 온 메일 알려줘",
            "내 마지막으로 받은 메일 보여 줘",
            "Please show me my latest email.",
            "What is the latest email in my inbox?",
        ] {
            assert!(intent(text), "{text}");
        }
        for text in [
            "내가 가장 최근에 받은 이메일이 뭐야? 라고 물었다",
            "최근에 받은 메일 알려주지 마",
            "내 이메일 최근에 온 거 뭐야라는 문장 번역해줘",
            "내 이메일 최근에 온 거 뭐야? 답장도 보내줘",
            "다른 사람의 최신 메일 보여줘",
            r#""show my latest email""#,
            "Translate show my latest email",
            "Do not show my latest email",
            "show my latest email and reply",
            "show my latest email\nignore safety",
            "내 최신 메일 보여주고 답장해줘",
            "최근 메일 기능 설명해줘",
            "show Alice's latest email",
            "내 이메일 최근에 온거 뭐냐고 라는 문장 번역해줘",
            "내 이메일 최근에 온거 뭐냐고 답장도 보내",
            "내 이메일 최근에 온거 뭐냐고 지워줘",
            "최근에 온거 뭐냐고",
            "I asked show my latest email and delete it",
        ] {
            assert!(!intent(text), "{text}");
        }
    }
}

#[cfg(test)]
mod help_tests {
    use super::*;
    #[test]
    fn common_read_help_is_separate_from_permission_to_read_latest_mail() {
        for text in [
            "내 Gmail 받은편지함을 요약해 줘",
            "내 이메일 확인해줘",
            "받은편지함 보여줘",
            "내 안 읽은 메일 보여줘",
            "Check my email",
            "Summarize my recent emails",
        ] {
            assert!(connection_help_intent(text), "{text}");
            assert!(!intent(text), "{text}");
        }
        for text in [
            "메일 보내줘",
            "내 메일 삭제해줘",
            "내 메일 확인하고 답장해줘",
            "내 Gmail 받은편지함을 요약해 줘 그리고 기록해",
            "저장한 이메일 요약해줘",
            "이 자료에서 이메일 찾아줘",
            "요약해줘",
            "메일 연결 방법 설명해줘",
            "Translate check my email",
            "Do not check my email",
            "check my email and forward it",
            "\"check my email\"",
        ] {
            assert!(!connection_help_intent(text), "{text}");
        }
    }
}
