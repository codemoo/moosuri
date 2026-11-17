//! Paired Telegram is a transport for existing account authority, never a new agent.
use super::*;
use moosuri_connectors::telegram::{Client, Error as TransportError, Identity, Update};
use serde::{Deserialize, Serialize};
mod delivery;
mod incoming;
pub use delivery::Dispatch;
#[cfg(test)]
mod tests;

#[derive(Serialize)]
pub struct BotStatus {
    pub revision: i64,
    pub username: Option<String>,
    pub enabled: bool,
    pub status: String,
    pub last_poll_at: Option<i64>,
}
#[derive(Serialize)]
pub struct BindingView {
    pub id: String,
    pub telegram_user_id: i64,
    pub thread_id: Option<String>,
    pub notifications: bool,
    pub timezone: String,
    pub last_delivery: Option<String>,
}
#[derive(Serialize)]
pub struct Settings {
    pub bot: BotStatus,
    pub binding: Option<BindingView>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PairInput {
    pub timezone: String,
    pub notifications: bool,
}
#[derive(Serialize)]
pub struct Pairing {
    pub url: String,
    pub code: String,
    pub expires_at: i64,
}
pub struct BotRuntime {
    pub revision: i64,
    pub offset: i64,
    pub client: Client,
}
struct Binding {
    id: String,
    user: UserContext,
    chat: i64,
    thread: Option<String>,
    timezone: String,
}

fn status(c: &Connection) -> Result<BotStatus> {
    Ok(c.query_row("SELECT revision,username,token IS NOT NULL,status,last_poll_at FROM telegram_bot WHERE singleton=1", [], |r| Ok(BotStatus { revision:r.get(0)?,username:r.get(1)?,enabled:r.get(2)?,status:r.get(3)?,last_poll_at:r.get(4)? }))?)
}
fn guard_bot(c: &Connection, revision: i64) -> Result<()> {
    let bot = status(c)?;
    if !bot.enabled
        || bot.revision != revision
        || matches!(bot.status.as_str(), "conflict" | "unauthorized")
    {
        return Err(StoreError::Conflict);
    }
    Ok(())
}
fn binding(c: &Connection, id: &str, revision: i64, renew: bool) -> Result<Binding> {
    guard_bot(c, revision)?;
    let (tenant,account,d,chat,thread,timezone):(String,String,Vec<u8>,i64,Option<String>,String)=c.query_row("SELECT tenant_id,account_id,session_digest,chat_id,thread_id,timezone FROM telegram_bindings WHERE id=?1 AND bot_revision=?2",params![id,revision],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))).optional()?.ok_or(StoreError::Unauthorized)?;
    let user = load_context(c, d.try_into().map_err(|_| StoreError::Corrupt)?, renew)?;
    if user.restricted || user.tenant.0 != tenant || user.account.id != account {
        return Err(StoreError::Unauthorized);
    }
    Ok(Binding {
        id: id.into(),
        user,
        chat,
        thread,
        timezone,
    })
}
fn unlink(c: &Connection, tenant: &str, account: &str) -> Result<()> {
    c.execute(
        "DELETE FROM telegram_pairings WHERE tenant_id=?1 AND account_id=?2",
        params![tenant, account],
    )?;
    c.execute("DELETE FROM sessions WHERE digest IN(SELECT session_digest FROM telegram_bindings WHERE tenant_id=?1 AND account_id=?2)",params![tenant,account])?;
    Ok(())
}
pub(crate) fn quarantine(c: &Connection) -> Result<()> {
    c.execute(
        "DELETE FROM sessions WHERE digest IN(SELECT session_digest FROM telegram_bindings)",
        [],
    )?;
    c.execute("DELETE FROM telegram_pairings", [])?;
    // Explicit deletion also applies when a restore has disabled foreign keys.
    c.execute_batch("DELETE FROM telegram_outbox; DELETE FROM telegram_turns; DELETE FROM telegram_bindings; UPDATE telegram_bot SET revision=revision+1,token=NULL,status='disabled',offset=0,last_poll_at=NULL;")?;
    Ok(())
}
pub(crate) fn restart(c: &Connection) -> Result<()> {
    c.execute("UPDATE telegram_bindings SET last_delivery='unknown' WHERE id IN(SELECT binding_id FROM telegram_outbox WHERE state='dispatched')",[])?;
    c.execute(
        "UPDATE telegram_outbox SET state='unknown',body=NULL WHERE state='dispatched'",
        [],
    )?;
    Ok(())
}
/// A web retry remains part of the same paired conversation, with a distinct
/// attempt receipt. Expired/revoked links still fail final dispatch authorization.
pub(crate) fn track_retry(c: &Connection, r: &crate::chat::Run) -> Result<()> {
    c.execute("INSERT OR IGNORE INTO telegram_turns(tenant_id,binding_id,run_id,attempt) SELECT t.tenant_id,t.binding_id,t.run_id,?3 FROM telegram_turns t JOIN telegram_bindings b ON b.id=t.binding_id WHERE t.tenant_id=?1 AND t.run_id=?2 AND t.attempt<?3",params![r.tenant,r.id,r.attempt])?;
    c.execute(
        "UPDATE telegram_turns SET settled=1 WHERE tenant_id=?1 AND run_id=?2 AND attempt<?3",
        params![r.tenant, r.id, r.attempt],
    )?;
    Ok(())
}
fn aad(revision: i64, bot: i64) -> Vec<u8> {
    format!("moosuri/telegram/token/v1/{revision}/{bot}").into_bytes()
}
fn client(db: &Database, token: Zeroizing<String>) -> Result<Client> {
    #[allow(unused_mut)]
    let mut client = Client::new(token).map_err(|_| StoreError::InvalidInput)?;
    #[cfg(feature = "test-fixtures")]
    if let Some(base) = &db.telegram_fixture {
        client = client.fixture(base).map_err(|_| StoreError::InvalidInput)?;
    }
    #[cfg(not(feature = "test-fixtures"))]
    let _ = db;
    Ok(client)
}
impl Store {
    pub async fn telegram_client(&self, token: Zeroizing<String>) -> Result<Client> {
        self.call(move |db| client(db, token)).await
    }
    #[cfg(feature = "test-fixtures")]
    pub async fn telegram_test_endpoint(&self, base: String) -> Result<()> {
        self.call(move |db| {
            Client::new(Zeroizing::new(
                "12345:synthetic_token_for_tests_only".into(),
            ))
            .map_err(|_| StoreError::InvalidInput)?
            .fixture(&base)
            .map_err(|_| StoreError::InvalidInput)?;
            db.telegram_fixture = Some(base);
            Ok(())
        })
        .await
    }
    pub async fn telegram_settings(&self, user: &UserContext, admin: bool) -> Result<Settings> {
        let user = user.clone();
        self.call(move|db|{
            let u=validate(&db.conn,&user,true,admin)?;let bot=status(&db.conn)?;
            let view=db.conn.query_row("SELECT id,telegram_user_id,thread_id,notifications,timezone,last_delivery FROM telegram_bindings WHERE tenant_id=?1 AND account_id=?2",params![u.tenant.0,u.account.id],|r|Ok(BindingView{id:r.get(0)?,telegram_user_id:r.get(1)?,thread_id:r.get(2)?,notifications:r.get(3)?,timezone:r.get(4)?,last_delivery:r.get(5)?})).optional()?;
            let view=view.filter(|b|binding(&db.conn,&b.id,bot.revision,false).is_ok());
            Ok(Settings{bot,binding:view})
        }).await
    }
    /// The HTTP boundary verifies the identity using Telegram before calling this method.
    pub async fn configure_telegram(
        &self,
        user: &UserContext,
        expected: i64,
        identity: Identity,
        token: Zeroizing<String>,
    ) -> Result<()> {
        Client::new(token.clone()).map_err(|_| StoreError::InvalidInput)?;
        let user = user.clone();
        self.call(move|db|{
            validate(&db.conn,&user,true,true)?;let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            if status(&tx)?.revision!=expected || !identity.is_bot || identity.id<=0 {return Err(StoreError::Conflict)}
            let username=identity.username.ok_or(StoreError::InvalidInput)?;
            if username.is_empty() || username.len()>64 || !username.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'_'){return Err(StoreError::InvalidInput)}
            quarantine(&tx)?;let revision=expected.checked_add(1).ok_or(StoreError::InvalidInput)?;
            let cipher=providers::encrypt(&db.provider_key,token.as_bytes(),&aad(revision,identity.id))?;
            tx.execute("UPDATE telegram_bot SET revision=?1,bot_id=?2,username=?3,token=?4,status='connected' WHERE singleton=1",params![revision,identity.id,username,cipher])?;
            tx.commit()?;Ok(())
        }).await
    }
    pub async fn disable_telegram(&self, user: &UserContext, expected: i64) -> Result<()> {
        let user = user.clone();
        self.call(move |db| {
            validate(&db.conn, &user, true, true)?;
            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            if status(&tx)?.revision != expected {
                return Err(StoreError::Conflict);
            }
            quarantine(&tx)?;
            tx.commit()?;
            Ok(())
        })
        .await
    }
    pub async fn pair_telegram(&self, user: &UserContext, input: PairInput) -> Result<Pairing> {
        crate::schedules::chat::validate_timezone(&input.timezone)?;
        let user = user.clone();
        self.call(move|db|{
            let u=validate(&db.conn,&user,true,false)?;let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let bot=status(&tx)?;guard_bot(&tx,bot.revision)?;
            let code=random_token()?;let expires_at=now()?+600;
            tx.execute("DELETE FROM telegram_pairings WHERE expires_at<=?1",[now()?])?;
            tx.execute("INSERT INTO telegram_pairings VALUES(?1,?2,?3,?4,?5,?6,?7,?8) ON CONFLICT(tenant_id,account_id) DO UPDATE SET digest=excluded.digest,session_digest=excluded.session_digest,bot_revision=excluded.bot_revision,expires_at=excluded.expires_at,timezone=excluded.timezone,notifications=excluded.notifications",params![u.tenant.0,u.account.id,digest(&code).as_slice(),u.digest.as_slice(),bot.revision,expires_at,input.timezone,input.notifications])?;
            let result=Pairing{url:format!("https://t.me/{}?start={code}",bot.username.ok_or(StoreError::Corrupt)?),code,expires_at};tx.commit()?;Ok(result)
        }).await
    }
    pub async fn disconnect_telegram(&self, user: &UserContext) -> Result<()> {
        let user = user.clone();
        self.call(move |db| {
            let u = validate(&db.conn, &user, true, false)?;
            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            unlink(&tx, &u.tenant.0, &u.account.id)?;
            tx.commit()?;
            Ok(())
        })
        .await
    }
    pub async fn telegram_notifications(
        &self,
        user: &UserContext,
        id: String,
        enabled: bool,
    ) -> Result<()> {
        let user = user.clone();
        self.call(move|db|{let u=validate(&db.conn,&user,true,false)?;let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;let n=tx.execute("UPDATE telegram_bindings SET notifications=?4,notification_cursor=(SELECT coalesce(max(rowid),0) FROM local_notifications) WHERE tenant_id=?1 AND account_id=?2 AND id=?3",params![u.tenant.0,u.account.id,id,enabled])?;if n!=1{return Err(StoreError::NotFound)}tx.execute("UPDATE telegram_outbox SET state='cancelled' WHERE binding_id=?1 AND notification_id IS NOT NULL AND state='queued'",[id])?;tx.commit()?;Ok(())}).await
    }
    pub async fn telegram_runtime(&self) -> Result<Option<BotRuntime>> {
        self.call(move |db| {
            let bot = status(&db.conn)?;
            if !bot.enabled || matches!(bot.status.as_str(), "conflict" | "unauthorized") {
                return Ok(None);
            }
            let (id, offset, cipher): (i64, i64, Vec<u8>) = db.conn.query_row(
                "SELECT bot_id,offset,token FROM telegram_bot WHERE singleton=1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?;
            let token = providers::decrypt(&db.provider_key, &cipher, &aad(bot.revision, id))?;
            let client = client(
                db,
                Zeroizing::new(String::from_utf8(token.to_vec()).map_err(|_| StoreError::Corrupt)?),
            )?;
            Ok(Some(BotRuntime {
                revision: bot.revision,
                offset,
                client,
            }))
        })
        .await
    }
    pub async fn telegram_poll_result(
        &self,
        revision: i64,
        error: Option<TransportError>,
    ) -> Result<()> {
        self.call(move|db|{let state=match error{None=>"polling",Some(TransportError::Unauthorized)=>"unauthorized",Some(TransportError::Conflict)=>"conflict",Some(TransportError::Retry(_))=>"rate_limited",_=>"error"};db.conn.execute("UPDATE telegram_bot SET status=?2,last_poll_at=?3 WHERE singleton=1 AND revision=?1 AND token IS NOT NULL AND status NOT IN('conflict','unauthorized')",params![revision,state,now()?])?;Ok(())}).await
    }
}
