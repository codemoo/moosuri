//! Literal latest-INBOX answer; never passes mail to any model.
use super::*;

fn connection_guidance(
    configured: bool,
    admin: bool,
    connections: &[moosuri_core::MailConnection],
) -> Option<(&'static str, moosuri_core::MailConnectionRecovery)> {
    use moosuri_core::MailConnectionRecovery::*;
    let active = connections
        .iter()
        .filter(|c| !c.native_send && c.status == "active")
        .count();
    if !configured {
        if admin {
            Some((
                "이 설치에는 Google 앱이 아직 준비되지 않아 Gmail을 확인하지 못했어요. 관리자인 본인이 Google 설치 설정에서 이 서버의 OAuth 앱을 먼저 설정한 뒤, Gmail 설정에서 본인 계정의 읽기 권한을 연결해 주세요. 연결을 마치면 이 대화에서 조회를 이어갈 수 있어요.",
                AdminSetup,
            ))
        } else {
            Some((
                "이 설치에는 Google 앱이 아직 준비되지 않아 Gmail을 확인하지 못했어요. 설치 관리자에게 Google 앱 설정을 요청해 주세요. 준비가 끝나면 Gmail 설정에서 본인 Google 계정으로 로그인하고 읽기 권한을 연결해 주세요.",
                MemberSetup,
            ))
        }
    } else if active > 1 {
        Some((
            "Gmail 읽기 계정이 여러 개 연결되어 있어 어느 받은편지함을 읽을지 정할 수 없어요. Gmail 설정에서 연결된 계정을 확인해 주세요. 현재 간편 조회는 여러 계정 중 하나를 선택할 수 없어 메일을 읽지 않았어요.",
            ChooseAccount,
        ))
    } else if active == 0
        && connections
            .iter()
            .any(|c| !c.native_send && c.status == "needs_reconnect")
    {
        Some((
            "Gmail 읽기 연결을 다시 인증해야 해요. Gmail 설정에서 본인 계정을 다시 연결해 주세요. 지금은 메일을 확인하지 못했어요.",
            Reconnect,
        ))
    } else if active == 0 {
        Some((
            "현재 로그인한 계정에 Gmail 읽기 연결이 없어요. Gmail 설정에서 본인 Google 계정으로 로그인하고 읽기 권한을 허용해 주세요. 기기의 미리보기·발송 전용 연결만으로는 이 조회를 할 수 없어요.",
            Connect,
        ))
    } else {
        None
    }
}
impl MailService {
    pub(crate) async fn latest_inbox_chat(
        &self,
        lease: &moosuri_store::RunLease,
    ) -> Result<bool, StoreError> {
        let Some((user, connections, latest)) = self.store.latest_inbox_request(lease).await?
        else {
            return Ok(false);
        };
        if let Some((id, epoch)) = self.store.gmail_server_resume_pin(lease).await?
            && !connections
                .iter()
                .any(|c| c.id == id && c.epoch == epoch && !c.native_send && c.status == "active")
        {
            return Err(StoreError::Conflict);
        }
        if let Some(handled) = self.gws_latest_chat(lease, &user, latest).await? {
            return Ok(handled);
        }
        // Preserve an explicitly admitted native read when no server connection
        // exists. A fresh user's setup and any server-pinned continuation use
        // server OAuth; having an optional device grant never steals that route.
        if !connections
            .iter()
            .any(|c| !c.native_send && c.status == "active")
            && self
                .store
                .mail_model_connection_context(lease)
                .await?
                .native_read_admitted
        {
            return Ok(false);
        }
        if !latest
            && connections
                .iter()
                .any(|c| !c.native_send && c.status == "active")
        {
            return Ok(false);
        }
        let configured = self.configured().await;
        if let Some((guidance, recovery)) = connection_guidance(
            configured,
            user.account().role == moosuri_core::Role::Admin,
            &connections,
        ) {
            let _delivery = self.signals.delivery.write().await;
            self.store
                .finish_mail_connection_guidance(lease, guidance.into(), recovery)
                .await?;
            return Ok(true);
        }
        if !latest {
            return Ok(false);
        }
        let (text, binding) = {
            let c = connections
                .iter()
                .find(|c| !c.native_send && c.status == "active")
                .ok_or(StoreError::Conflict)?;
            match self.latest_inbox_read(lease, &user, c).await {
                Ok((text, binding)) => (text, Some(binding)),
                Err(_) => ("Gmail 받은편지함을 확인하지 못했어요. 설정 → Gmail (/settings/mail)에서 현재 계정의 연결과 읽기 권한을 확인해 주세요. 최신 메일이나 받은편지함이 비었다고 확인한 것은 아니에요.".into(), None),
            }
        };
        let _delivery = self.signals.delivery.write().await;
        self.store
            .finish_latest_inbox(lease, text, binding.as_ref())
            .await?;
        Ok(true)
    }
    async fn latest_inbox_read(
        &self,
        lease: &moosuri_store::RunLease,
        user: &UserContext,
        c: &moosuri_core::MailConnection,
    ) -> Result<(String, MailBinding), StoreError> {
        self.store
            .pin_latest_inbox(lease, c.id.clone(), c.epoch)
            .await?;
        let cancel = self.signals.stopping().child_token();
        let b = self.binding(user, &c.id, &cancel).await?;
        if b.epoch != c.epoch {
            return Err(StoreError::Conflict);
        }
        let mut page_token = None;
        let mut seen = std::collections::HashSet::new();
        let mut newest: Option<moosuri_connectors::gmail::InboxMetadata> = None;
        let mut exhausted = false;
        // Gmail list order is unspecified. Only an exhausted bounded scan can
        // identify the maximum observed internalDate over the whole INBOX.
        for _ in 0..4 {
            self.store.check_run(lease).await?;
            let page = self
                .client
                .list_messages(
                    &b.access_token,
                    "in:inbox",
                    page_token.as_deref(),
                    5,
                    &cancel,
                )
                .await
                .map_err(|_| StoreError::Unavailable)?;
            for item in page.messages {
                if !seen.insert(item.id.clone()) {
                    return Err(StoreError::Unavailable);
                }
                self.store.check_run(lease).await?;
                let m = self
                    .client
                    .inbox_metadata(&b.access_token, &item.id, &cancel)
                    .await
                    .map_err(|_| StoreError::Unavailable)?;
                if newest
                    .as_ref()
                    .is_none_or(|old| (m.received_at_ms, &m.id) > (old.received_at_ms, &old.id))
                {
                    newest = Some(m);
                }
            }
            page_token = page.next_page_token;
            if page_token.is_none() {
                exhausted = true;
                break;
            }
        }
        // Revalidate the binding and Google app after IO; lease dependencies
        // and the final transaction also fence cancellation/disconnection.
        let current = self.store.mail_binding(user, b.id.clone()).await?;
        if current.epoch != b.epoch {
            return Err(StoreError::Conflict);
        }
        let pin = b.app_pin().cloned().ok_or(StoreError::Unavailable)?;
        let current = self.store.pin_mail_binding(current, pin).await?;
        self.store.check_run(lease).await?;
        Ok((render(newest.as_ref(), seen.len(), exhausted), current))
    }
}
fn render(
    m: Option<&moosuri_connectors::gmail::InboxMetadata>,
    count: usize,
    exhausted: bool,
) -> String {
    let Some(m) = m else {
        return if exhausted {
            "조회 시점의 Gmail 받은편지함에 메일이 없어요.".into()
        } else {
            "제한된 조회에서 메일을 확인하지 못했어요. 받은편지함 전체가 비었다고 확인한 것은 아니에요.".into()
        };
    };
    let status = if exhausted {
        "조회한 받은편지함 전체에서 가장 최근 수신 시각의 메일이에요. 조회 도중 새 메일이 도착할 수 있어요.".to_owned()
    } else {
        format!(
            "최대 20개 제한 중 {count}개를 확인했고 추가 페이지가 남아 있어요. 아래는 확인한 범위에서 가장 최근 메일이며, 받은편지함 전체의 최신 메일인지는 확인하지 못했어요."
        )
    };

    format!(
        "{status}\n제목: {}\n보낸 사람: {}\n메일 날짜: {}\n본문은 읽지 않았어요.",
        m.subject, m.from, m.date
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    fn connection(status: &str) -> moosuri_core::MailConnection {
        moosuri_core::MailConnection {
            id: "synthetic".into(),
            email: "synthetic@example.test".into(),
            status: status.into(),
            epoch: 0,
            native_send: false,
            compose_enabled: false,
        }
    }
    #[test]
    fn actionable_missing_ambiguous_and_disconnected_accounts() {
        assert!(
            connection_guidance(false, false, &[])
                .unwrap()
                .0
                .contains("Google 앱")
        );
        assert!(
            connection_guidance(true, false, &[])
                .unwrap()
                .0
                .contains("읽기 연결이 없어요")
        );
        assert!(
            connection_guidance(true, false, &[connection("needs_reconnect")])
                .unwrap()
                .0
                .contains("다시 인증")
        );
        assert!(
            connection_guidance(true, false, &[connection("active"), connection("active")])
                .unwrap()
                .0
                .contains("여러 개")
        );
        assert!(connection_guidance(true, false, &[connection("active")]).is_none());
    }
    #[test]
    fn bounded_scan_never_claims_latest_without_exhaustion() {
        let m = moosuri_connectors::gmail::InboxMetadata {
            id: "synthetic".into(),
            received_at_ms: 100,
            subject: "literal".into(),
            from: "synthetic@example.test".into(),
            date: "date".into(),
        };
        assert!(render(Some(&m), 20, false).contains("최신 메일인지는 확인하지 못했어요"));
        assert!(render(Some(&m), 1, true).contains("전체에서 가장 최근"));
        assert!(render(None, 0, true).contains("메일이 없어요"));
    }
}
