//! Content-free owner capability for model context, independent of literal intents.
use super::*;

pub struct MailModelConnectionContext {
    pub admin: bool,
    pub server_read_connected: bool,
    pub server_read_admitted: bool,
    pub needs_reconnect: bool,
    pub native_read_admitted: bool,
}
impl Store {
    /// One current run/owner boundary; no credentials, addresses, mailbox contents
    /// or other accounts are returned, and this observation grants no mail IO.
    pub async fn mail_model_connection_context(
        &self,
        lease: &crate::RunLease,
    ) -> Result<MailModelConnectionContext> {
        let lease = lease.clone();
        self.call(move |db| {
            let run = crate::chat::guard(&db.conn, &db.provider_key, &lease)?;
            connection_context(&db.conn, &run)
        })
        .await
    }
}

pub(super) fn connection_context(
    conn: &Connection,
    run: &crate::chat::Run,
) -> Result<MailModelConnectionContext> {
    let user = crate::chat::authority(conn, run)?;
    let rows = conn.prepare("SELECT id,epoch,status,scopes FROM mail_connections WHERE tenant_id=?1 AND account_id=?2 AND native_send=0 LIMIT 16")?
                .query_map(params![run.tenant, run.account], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut context = MailModelConnectionContext {
        admin: user.account.role == Role::Admin,
        server_read_connected: false,
        server_read_admitted: false,
        needs_reconnect: false,
        native_read_admitted: crate::native_task_mail::current_model_read_admitted(conn, run)?,
    };
    for (id, epoch, status, scopes) in rows {
        let readable = parse::<Vec<String>>(&scopes)?
            .iter()
            .any(|s| s == moosuri_connectors::gmail::READ_SCOPE);
        context.needs_reconnect |= readable && status == "needs_reconnect";
        if readable && status == "active" {
            context.server_read_connected = true;
            context.server_read_admitted |= run.tool_profile.as_ref().is_some_and(|p| {
                p.mail_connections
                    .iter()
                    .any(|pin| !pin.native_send && pin.id == id && pin.epoch == epoch)
            });
        }
    }
    Ok(context)
}
