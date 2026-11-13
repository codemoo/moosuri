//! Durable ownership and controller fencing. All browser IO lives outside this actor.
use super::*;
use moosuri_core::{BrowserControl, BrowserWorkspaceState as State, BrowserWorkspaceView};

/// Constructed only by the trusted broker, never deserialized as a model request.
#[derive(Clone)]
pub enum BrowserAccess {
    Watch,
    Takeover,
    AgentObserve,
    AgentNavigate,
    Human(String),
}

pub(super) fn restart(conn: &Connection) -> Result<()> {
    conn.execute("UPDATE browser_workspaces SET state='outcome_unknown',revision=revision+1,control_hash=NULL,control_session=NULL,control_until=NULL,url=NULL,title=NULL,reason=NULL WHERE state IN('starting','ready','waiting_for_user','user_control')", [])?;
    Ok(())
}

type ResumeRow = (
    i64,
    String,
    Option<String>,
    Option<Vec<u8>>,
    Option<Vec<u8>>,
);

fn thread(conn: &Connection, user: &UserContext, id: &str) -> Result<()> {
    validate(conn, user, true, false)?;
    if !conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM threads WHERE tenant_id=?1 AND account_id=?2 AND id=?3)",
        params![user.tenant.0, user.account.id, id],
        |r| r.get::<_, bool>(0),
    )? {
        return Err(StoreError::NotFound);
    }
    Ok(())
}

fn load(
    conn: &Connection,
    user: &UserContext,
    thread_id: &str,
) -> Result<Option<BrowserWorkspaceView>> {
    thread(conn, user, thread_id)?;
    let time = now()?;
    conn.execute("UPDATE browser_workspaces SET state='expired',revision=revision+1,control_hash=NULL,control_session=NULL,control_until=NULL,url=NULL,title=NULL,reason=NULL WHERE tenant_id=?1 AND thread_id=?2 AND state IN('starting','ready','waiting_for_user','user_control') AND (expires_at<=?3 OR auth_epoch!=?4)",params![user.tenant.0,thread_id,time,user.epoch])?;
    // Lease expiry never gives the model control. An explicit completion is required.
    conn.execute("UPDATE browser_workspaces SET state='waiting_for_user',revision=revision+1,control_hash=NULL,control_session=NULL,control_until=NULL WHERE tenant_id=?1 AND thread_id=?2 AND state='user_control' AND (control_until<=?3 OR NOT EXISTS(SELECT 1 FROM sessions WHERE digest=control_session AND auth_epoch=?4 AND expires_at>?3))",params![user.tenant.0,thread_id,time,user.epoch])?;
    let row = conn.query_row("SELECT id,state,revision,human_touched,url,title,reason,expires_at FROM browser_workspaces WHERE tenant_id=?1 AND account_id=?2 AND thread_id=?3",params![user.tenant.0,user.account.id,thread_id],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?))).optional()?;
    row.map(|r| {
        Ok(BrowserWorkspaceView {
            id: r.0,
            thread_id: thread_id.to_owned(),
            state: serde_json::from_value(serde_json::Value::String(r.1))
                .map_err(|_| StoreError::Corrupt)?,
            revision: r.2,
            human_touched: r.3,
            url: r.4,
            title: r.5,
            reason: r.6,
            expires_at: r.7,
        })
    })
    .transpose()
}

fn checked(
    conn: &Connection,
    user: &UserContext,
    thread_id: &str,
    id: &str,
    revision: i64,
    access: &BrowserAccess,
) -> Result<BrowserWorkspaceView> {
    let view = load(conn, user, thread_id)?.ok_or(StoreError::NotFound)?;
    if view.id != id || view.revision != revision {
        return Err(StoreError::Conflict);
    }
    match access {
        BrowserAccess::Watch if matches!(view.state, State::Ready | State::WaitingForUser) => {}
        BrowserAccess::Takeover
            if matches!(
                view.state,
                State::Ready | State::WaitingForUser | State::UserControl
            ) => {}
        BrowserAccess::AgentObserve if view.state == State::Ready => {}
        BrowserAccess::AgentNavigate if view.state == State::Ready && !view.human_touched => {}
        BrowserAccess::Human(token) if view.state == State::UserControl && token.len() == 64 => {
            let valid=conn.query_row("SELECT control_hash=?4 AND control_session=?5 AND control_until>?6 FROM browser_workspaces WHERE tenant_id=?1 AND thread_id=?2 AND id=?3",params![user.tenant.0,thread_id,id,digest(token).as_slice(),user.digest.as_slice(),now()?],|r|r.get::<_,bool>(0))?;
            if !valid {
                return Err(StoreError::Forbidden);
            }
        }
        _ => return Err(StoreError::Conflict),
    }
    Ok(view)
}

fn location(url: &str, title: &str) -> Result<()> {
    let parsed = url::Url::parse(url).map_err(|_| StoreError::InvalidInput)?;
    if url.len() > 2048
        || title.len() > 512
        || !matches!(parsed.scheme(), "http" | "https")
        || parsed.host().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return Err(StoreError::InvalidInput);
    }
    Ok(())
}

impl Store {
    pub async fn browser_workspace(
        &self,
        user: &UserContext,
        thread_id: String,
    ) -> Result<Option<BrowserWorkspaceView>> {
        let user = user.clone();
        self.call(move |db| load(&db.conn, &user, &thread_id)).await
    }
    pub async fn begin_browser_workspace(
        &self,
        user: &UserContext,
        thread_id: String,
        pin: String,
    ) -> Result<BrowserWorkspaceView> {
        let user = user.clone();
        self.call(move|db|{
            thread(&db.conn,&user,&thread_id)?;
            if db.web_profile.browser_pin.as_deref()!=Some(&pin) {return Err(StoreError::Conflict)}
            if let Some(old)=load(&db.conn,&user,&thread_id)? && !matches!(old.state,State::Closed|State::Expired|State::OutcomeUnknown) {return Err(StoreError::Conflict)}
            let id=Uuid::new_v4().to_string();let time=now()?;
            db.conn.execute("INSERT INTO browser_workspaces(tenant_id,account_id,thread_id,id,auth_epoch,deployment_pin,state,created_at,expires_at) VALUES(?1,?2,?3,?4,?5,?6,'starting',?7,?8) ON CONFLICT(tenant_id,thread_id) DO UPDATE SET id=excluded.id,auth_epoch=excluded.auth_epoch,deployment_pin=excluded.deployment_pin,state='starting',revision=1,human_touched=0,url=NULL,title=NULL,reason=NULL,created_at=excluded.created_at,expires_at=excluded.expires_at,control_hash=NULL,control_session=NULL,control_until=NULL,resume_request=NULL,resumed_request=NULL",params![user.tenant.0,user.account.id,thread_id,id,user.epoch,pin,time,time+8*60*60])?;
            load(&db.conn,&user,&thread_id)?.ok_or(StoreError::Corrupt)
        }).await
    }
    pub async fn activate_browser_workspace(
        &self,
        user: &UserContext,
        thread_id: String,
        id: String,
        url: String,
        title: String,
    ) -> Result<BrowserWorkspaceView> {
        location(&url, &title)?;
        let user = user.clone();
        self.call(move|db|{
            thread(&db.conn,&user,&thread_id)?;
            let changed=db.conn.execute("UPDATE browser_workspaces SET state='ready',revision=revision+1,url=?4,title=?5 WHERE tenant_id=?1 AND thread_id=?2 AND id=?3 AND state='starting' AND auth_epoch=?6 AND expires_at>?7",params![user.tenant.0,thread_id,id,url,title,user.epoch,now()?])?;
            if changed!=1{return Err(StoreError::Conflict)}
            load(&db.conn,&user,&thread_id)?.ok_or(StoreError::Corrupt)
        }).await
    }
    pub async fn check_browser_workspace(
        &self,
        user: &UserContext,
        thread_id: String,
        id: String,
        revision: i64,
        access: BrowserAccess,
    ) -> Result<BrowserWorkspaceView> {
        let user = user.clone();
        self.call(move |db| checked(&db.conn, &user, &thread_id, &id, revision, &access))
            .await
    }
    pub async fn browser_location(
        &self,
        user: &UserContext,
        view: BrowserWorkspaceView,
        access: BrowserAccess,
        url: String,
        title: String,
    ) -> Result<BrowserWorkspaceView> {
        location(&url, &title)?;
        let user = user.clone();
        self.call(move|db|{
            checked(&db.conn,&user,&view.thread_id,&view.id,view.revision,&access)?;
            db.conn.execute("UPDATE browser_workspaces SET url=?4,title=?5,control_until=CASE WHEN state='user_control' THEN ?6 ELSE control_until END WHERE tenant_id=?1 AND thread_id=?2 AND id=?3",params![user.tenant.0,view.thread_id,view.id,url,title,now()?+300])?;
            load(&db.conn,&user,&view.thread_id)?.ok_or(StoreError::Corrupt)
        }).await
    }
    pub async fn browser_handoff(
        &self,
        user: &UserContext,
        view: BrowserWorkspaceView,
        reason: String,
    ) -> Result<BrowserWorkspaceView> {
        let reason = name_valid(reason, 500)?;
        let user = user.clone();
        self.call(move|db|{
            checked(&db.conn,&user,&view.thread_id,&view.id,view.revision,&BrowserAccess::AgentObserve)?;
            db.conn.execute("UPDATE browser_workspaces SET state='waiting_for_user',revision=revision+1,reason=?4 WHERE tenant_id=?1 AND thread_id=?2 AND id=?3",params![user.tenant.0,view.thread_id,view.id,reason])?;
            load(&db.conn,&user,&view.thread_id)?.ok_or(StoreError::Corrupt)
        }).await
    }
    pub async fn take_browser_control(
        &self,
        user: &UserContext,
        thread_id: String,
        id: String,
        revision: i64,
    ) -> Result<BrowserControl> {
        let user = user.clone();
        self.call(move|db|{
            checked(&db.conn,&user,&thread_id,&id,revision,&BrowserAccess::Takeover)?;
            let token=random_token()?;
            let request=Uuid::new_v4().to_string();
            db.conn.execute("UPDATE browser_workspaces SET state='user_control',human_touched=1,revision=revision+1,control_hash=?4,control_session=?5,control_until=?6,resume_request=?7,resumed_request=NULL WHERE tenant_id=?1 AND thread_id=?2 AND id=?3",params![user.tenant.0,thread_id,id,digest(&token).as_slice(),user.digest.as_slice(),now()?+300,request])?;
            Ok(BrowserControl{workspace:load(&db.conn,&user,&thread_id)?.ok_or(StoreError::Corrupt)?,control_token:token})
        }).await
    }
    /// Complete human control and enqueue its visible continuation in one commit.
    pub async fn resume_browser(
        &self,
        user: &UserContext,
        thread_id: String,
        id: String,
        revision: i64,
        token: String,
    ) -> Result<moosuri_core::ChatTurn> {
        let user = user.clone();
        self.call(move|db|{
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            thread(&tx,&user,&thread_id)?;
            let row:Option<ResumeRow>=tx.query_row("SELECT revision,state,resumed_request,control_hash,control_session FROM browser_workspaces WHERE tenant_id=?1 AND thread_id=?2 AND id=?3",params![user.tenant.0,thread_id,id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional()?;
            let resumed = match row {
                Some((rev,state,Some(request),Some(saved_digest),Some(session))) if state=="ready" && rev==revision+1 && saved_digest==digest(&token) && session==user.digest => Some(request),
                _=>None,
            };
            let request=if let Some(request)=resumed {request} else {
                checked(&tx,&user,&thread_id,&id,revision,&BrowserAccess::Human(token.clone()))?;
                let request:String=tx.query_row("SELECT resume_request FROM browser_workspaces WHERE tenant_id=?1 AND thread_id=?2 AND id=?3",params![user.tenant.0,thread_id,id],|r|r.get(0))?;
                tx.execute("UPDATE browser_workspaces SET state='ready',revision=revision+1,reason=NULL,control_until=NULL,resumed_request=resume_request WHERE tenant_id=?1 AND thread_id=?2 AND id=?3",params![user.tenant.0,thread_id,id])?;
                request
            };
            let turn=crate::chat::submit_turn_in(&tx,&db.provider_key,&db.web_profile,user,thread_id,request,"브라우저 직접 조작을 마쳤어. 현재 화면을 다시 확인하고 같은 브라우저에서 원래 요청을 이어서 처리해.".into())?;
            tx.commit()?;Ok(turn)
        }).await
    }
    pub async fn end_browser_workspace(
        &self,
        user: &UserContext,
        thread_id: String,
        id: String,
        revision: i64,
        state: State,
    ) -> Result<BrowserWorkspaceView> {
        if !matches!(
            state,
            State::Closed | State::Expired | State::OutcomeUnknown
        ) {
            return Err(StoreError::InvalidInput);
        }
        let user = user.clone();
        self.call(move|db|{
            let view=load(&db.conn,&user,&thread_id)?.ok_or(StoreError::NotFound)?;
            if view.id!=id || view.revision!=revision{return Err(StoreError::Conflict)}
            let state=serde_json::to_value(state).map_err(|_|StoreError::Corrupt)?;
            db.conn.execute("UPDATE browser_workspaces SET state=?4,revision=revision+1,control_hash=NULL,control_session=NULL,control_until=NULL,url=NULL,title=NULL,reason=NULL WHERE tenant_id=?1 AND thread_id=?2 AND id=?3",params![user.tenant.0,thread_id,id,state.as_str().ok_or(StoreError::Corrupt)?])?;
            load(&db.conn,&user,&thread_id)?.ok_or(StoreError::Corrupt)
        }).await
    }
}

#[cfg(test)]
mod tests;
