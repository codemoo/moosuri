//! Original-owner literal metadata route. Results never enter model history.
use super::*;
impl MailService {
    pub async fn gws_inbox_status(
        &self,
        user: &UserContext,
    ) -> Result<serde_json::Value, StoreError> {
        let binding = self
            .gws_inbox
            .as_ref()
            .map(|r| r.binding(&user.account().id))
            .transpose()
            .map_err(|_| StoreError::Unavailable)?
            .flatten();
        match binding {
            Some(binding) => Ok(
                serde_json::json!({"configured":true,"mailbox":binding.mailbox(),"confirmed":self.store.gws_inbox_confirmed(user,binding.fingerprint()).await?}),
            ),
            None => Ok(serde_json::json!({"configured":false,"confirmed":false})),
        }
    }
    /// Settings checks never list or read mail. A new binding always needs owner consent.
    pub async fn gws_inbox_readiness(
        &self,
        user: &UserContext,
    ) -> Result<serde_json::Value, StoreError> {
        self.gws_probe(user, None).await
    }
    pub async fn gws_inbox_confirm(
        &self,
        user: &UserContext,
        confirmation: String,
    ) -> Result<serde_json::Value, StoreError> {
        self.gws_probe(user, Some(confirmation)).await
    }
    async fn gws_probe(
        &self,
        user: &UserContext,
        confirmation: Option<String>,
    ) -> Result<serde_json::Value, StoreError> {
        let Some(registry) = &self.gws_inbox else {
            return Ok(serde_json::json!({"state":"unconfigured"}));
        };
        let binding = match registry.binding(&user.account().id) {
            Ok(Some(binding)) => binding,
            Ok(None) => return Ok(serde_json::json!({"state":"unconfigured"})),
            Err(_) => return Ok(serde_json::json!({"state":"offline"})),
        };
        let observed = self.store.gws_inbox_observation(user).await?;
        let state = registry
            .profile(&binding, &self.signals.stopping().child_token())
            .await
            .unwrap_or("offline");
        if state != "ready" {
            if state == "wrong_account" || state == "login_required" {
                self.store
                    .revoke_gws_inbox(user, binding.fingerprint())
                    .await?;
            }
            return Ok(serde_json::json!({"state":state,"mailbox":binding.mailbox()}));
        }
        registry.check(&binding).map_err(|_| StoreError::Conflict)?;
        if let Some(confirmation) = confirmation {
            self.store
                .confirm_gws_inbox(user, binding.fingerprint(), confirmation)
                .await?;
        }
        if self
            .store
            .gws_inbox_confirmed(user, binding.fingerprint())
            .await?
        {
            registry.check(&binding).map_err(|_| StoreError::Conflict)?;
            return Ok(serde_json::json!({"state":"ready","mailbox":binding.mailbox()}));
        }
        let challenge = self
            .store
            .gws_inbox_challenge_observed(user, binding.fingerprint(), observed)
            .await?;
        registry.check(&binding).map_err(|_| StoreError::Conflict)?;
        Ok(match challenge {
            Some(challenge) => {
                serde_json::json!({"state":"needs_confirmation","mailbox":binding.mailbox(),"confirmation":challenge})
            }
            None => serde_json::json!({"state":"ready","mailbox":binding.mailbox()}),
        })
    }
    /// Runtime-only pin for a resume; caller rechecks selection in its write transaction.
    pub async fn gws_inbox_resume_fingerprint(
        &self,
        user: &UserContext,
    ) -> Result<Option<[u8; 32]>, StoreError> {
        let Some(registry) = &self.gws_inbox else {
            return Ok(None);
        };
        let Some(binding) = registry
            .binding(&user.account().id)
            .map_err(|_| StoreError::Unavailable)?
        else {
            return Ok(None);
        };
        if registry
            .profile(&binding, &self.signals.stopping().child_token())
            .await
            .ok()
            != Some("ready")
        {
            return Ok(None);
        }
        if !self
            .store
            .gws_inbox_confirmed(user, binding.fingerprint())
            .await?
        {
            return Ok(None);
        }
        registry.check(&binding).map_err(|_| StoreError::Conflict)?;
        Ok(Some(binding.fingerprint()))
    }
    pub async fn gws_inbox_login(
        &self,
        user: &UserContext,
    ) -> Result<serde_json::Value, StoreError> {
        let Some(registry) = &self.gws_inbox else {
            return Ok(serde_json::json!({"state":"unconfigured"}));
        };
        let Some(binding) = registry
            .binding(&user.account().id)
            .map_err(|_| StoreError::Unavailable)?
        else {
            return Ok(serde_json::json!({"state":"unconfigured"}));
        };
        // Sign-in is an explicit settings action, and invalidates previous selection.
        self.store
            .revoke_gws_inbox(user, binding.fingerprint())
            .await?;
        if registry
            .login(&binding, &self.signals.stopping().child_token())
            .await
            .is_err()
        {
            return Ok(serde_json::json!({"state":"offline","mailbox":binding.mailbox()}));
        }
        Ok(serde_json::json!({"state":"login_started","mailbox":binding.mailbox()}))
    }
    /// Some(bool) means this explicit route owned the turn, even on failure.
    /// An offline/misconfigured selected helper cannot silently fall back to OAuth.
    pub(super) async fn gws_latest_chat(
        &self,
        lease: &moosuri_store::RunLease,
        user: &UserContext,
        latest: bool,
    ) -> Result<Option<bool>, StoreError> {
        let resumed_pin = self.store.gmail_resume_fingerprint(lease).await?;
        if resumed_pin.is_none() && !self.store.explicit_gws_latest_request(lease).await? {
            return Ok(None);
        }
        if let Some(pin) = resumed_pin {
            let current = self
                .gws_inbox
                .as_ref()
                .and_then(|r| r.binding(&user.account().id).ok().flatten());
            if current.is_none_or(|binding| binding.fingerprint() != pin) {
                self.finish_gws_unavailable(lease).await?;
                return Ok(Some(true));
            }
        }
        let Some(registry) = &self.gws_inbox else {
            return Ok(None);
        };
        match registry.mailbox(&user.account().id) {
            Ok(None) => return Ok(None),
            Ok(Some(_)) => {}
            Err(_) => {
                if !latest {
                    return Ok(Some(false));
                }
                self.finish_gws_unavailable(lease).await?;
                return Ok(Some(true));
            }
        }
        if !latest {
            return Ok(Some(false));
        }
        let result = async {
            let binding = registry.binding(&user.account().id).map_err(|_|StoreError::Unavailable)?.ok_or(StoreError::Conflict)?;
            if resumed_pin.is_some_and(|pin|pin!=binding.fingerprint()) {return Err(StoreError::Conflict);}
            if !self.store.gws_inbox_confirmed(user,binding.fingerprint()).await? {return Err(StoreError::Conflict);}
            self.store.check_run(lease).await?;
            let cancel = self.signals.stopping().child_token();
            let operation = registry.latest(&binding, &cancel);
            tokio::pin!(operation);
            let mut tick = tokio::time::interval(Duration::from_millis(250));
            let messages = loop {
                tokio::select! { biased;
                    _=self.signals.stopping().cancelled()=>return Err(StoreError::Unavailable),
                    _=tick.tick()=> {
                        self.store.check_run(lease).await?;
                        registry.check(&binding).map_err(|_|StoreError::Conflict)?;
                        if !self.store.gws_inbox_confirmed(user,binding.fingerprint()).await? {return Err(StoreError::Conflict);}
                    },
                    result=&mut operation=>break result.map_err(|_|StoreError::Unavailable)?,
                }
            };
            // Owner revocation, cancellation and original-request authorization
            // are checked after helper IO, before any literal output is stored.
            let current = self.store.latest_inbox_request(lease).await?.ok_or(StoreError::Forbidden)?;
            if !current.2 || current.0.account().id != user.account().id { return Err(StoreError::Forbidden); }
            let count = messages.len();
            let newest = messages.iter().max_by_key(|m|(m.internal_date,&m.id));
            let text = match newest {
                Some(m) => format!("GWS 연결 · {}\n받은편지함 최근 최대 20통 중 {count}통을 확인했고, 아래는 제공된 범위에서 가장 최근에 받은 메일이에요. 받은편지함 전체의 최신 메일인지는 확인하지 못했어요.\n제목: {}\n보낸 사람: {}\n메일 날짜: {}\n본문은 읽지 않았어요.",binding.mailbox(),m.subject,m.from,m.date),
                None => format!("GWS 연결 · {}\n도우미가 제공한 받은편지함 최근 메일 목록이 비어 있어요. 받은편지함 전체가 비었다고 확인한 것은 아니에요. 본문은 읽지 않았어요.",binding.mailbox()),
            };
            let _delivery = self.signals.delivery.write().await;
            registry.check(&binding).map_err(|_|StoreError::Conflict)?;
            if !self.store.gws_inbox_confirmed(user,binding.fingerprint()).await? {return Err(StoreError::Conflict);}
            self.store.finish_gws_latest_inbox(lease,text,binding.fingerprint()).await
        }.await;
        match result {
            Ok(()) => {}
            Err(StoreError::Unauthorized | StoreError::Forbidden | StoreError::BudgetExhausted) => {
                return result.map(|_| Some(true));
            }
            Err(_) => self.finish_gws_unavailable(lease).await?,
        }
        Ok(Some(true))
    }
    async fn finish_gws_unavailable(
        &self,
        lease: &moosuri_store::RunLease,
    ) -> Result<(), StoreError> {
        let _delivery = self.signals.delivery.write().await;
        self.store.finish_mail_connection_guidance(lease,"Gmail 연결을 확인하지 못했어요. 아래에서 본인 계정을 연결하면 이 대화에서 최근 메일 조회를 이어갈 수 있어요.".into(),moosuri_core::MailConnectionRecovery::GwsSetup).await
    }
}
