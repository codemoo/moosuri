//! Installation OAuth app secrets remain in trusted runtime carriers.
use super::*;
use moosuri_connectors::gmail::{OAuthClient, SecretString};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GoogleSetupRecord {
    pub revision: i64,
    pub client_id: String,
    pub redirect_uri: String,
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct GoogleAppPin {
    pub revision: Option<i64>,
    pub client_id: String,
    pub redirect_uri: String,
}
/// Trusted runtime only: intentionally neither Debug nor Serialize.
pub struct GoogleApp {
    pub client: Arc<OAuthClient>,
    pub pin: GoogleAppPin,
}
fn record(conn: &Connection) -> Result<Option<GoogleSetupRecord>> {
    Ok(conn
        .query_row(
            "SELECT revision,client_id,redirect_uri FROM google_setup WHERE singleton=1",
            [],
            |r| {
                Ok(GoogleSetupRecord {
                    revision: r.get(0)?,
                    client_id: r.get(1)?,
                    redirect_uri: r.get(2)?,
                })
            },
        )
        .optional()?)
}
fn secret_aad(record: &GoogleSetupRecord) -> Result<Vec<u8>> {
    serde_json::to_vec(&(
        "moosuri/google-setup/client-secret/v1",
        1,
        record.revision,
        &record.client_id,
        &record.redirect_uri,
    ))
    .map_err(|_| StoreError::InvalidInput)
}
fn validate_client(client: &OAuthClient) -> Result<()> {
    let url = url::Url::parse(&client.redirect_uri).map_err(|_| StoreError::InvalidInput)?;
    if client.client_id.trim().is_empty()
        || client.client_id.len() > 1024
        || client.client_id.chars().any(char::is_control)
        || client.redirect_uri.len() > 2048
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || url.query().is_some()
        || url.host_str().is_none()
        || !(url.scheme() == "https"
            || (url.scheme() == "http"
                && matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))))
    {
        return Err(StoreError::InvalidInput);
    }
    Ok(())
}
pub(crate) fn guard_app(conn: &Connection, pin: &GoogleAppPin) -> Result<()> {
    if let Some(revision) = pin.revision {
        let current = record(conn)?.ok_or(StoreError::Conflict)?;
        if current.revision != revision
            || current.client_id != pin.client_id
            || current.redirect_uri != pin.redirect_uri
        {
            return Err(StoreError::Conflict);
        }
    }
    Ok(())
}
pub(crate) fn guard_mail_app(
    conn: &Connection,
    tenant: &str,
    id: &str,
    pin: &GoogleAppPin,
) -> Result<()> {
    guard_app(conn, pin)?;
    let client: Option<String> = conn
        .query_row(
            "SELECT oauth_client_id FROM mail_connections WHERE tenant_id=?1 AND id=?2",
            params![tenant, id],
            |r| r.get(0),
        )
        .optional()?
        .ok_or(StoreError::NotFound)?;
    if client.as_ref().is_some_and(|c| c != &pin.client_id)
        || (client.is_none() && pin.revision.is_some())
    {
        return Err(StoreError::Conflict);
    }
    Ok(())
}
impl Store {
    pub async fn google_setup(&self, user: &UserContext) -> Result<Option<GoogleSetupRecord>> {
        let user = user.clone();
        self.call(move |db| {
            validate(&db.conn, &user, true, true)?;
            record(&db.conn)
        })
        .await
    }
    pub async fn save_google_setup(
        &self,
        user: &UserContext,
        expected_revision: i64,
        client: OAuthClient,
    ) -> Result<GoogleSetupRecord> {
        validate_client(&client)?;
        let user = user.clone();
        self.call(move|db|{
            validate(&db.conn,&user,true,true)?;
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let old=record(&tx)?;
            if old.as_ref().map_or(0,|r|r.revision)!=expected_revision {return Err(StoreError::Conflict)}
            let time=now()?;
            if tx.query_row("SELECT EXISTS(SELECT 1 FROM mail_oauth_sessions WHERE expires_at>?1) OR EXISTS(SELECT 1 FROM calendar_oauth_sessions WHERE expires_at>?1)",[time],|r|r.get::<_,bool>(0))? {return Err(StoreError::GoogleAuthorizationInProgress)}
            let identity_changed=old.as_ref().is_none_or(|r|r.client_id!=client.client_id);
            let redirect_changed=old.as_ref().is_some_and(|r|r.redirect_uri!=client.redirect_uri);
            if redirect_changed && tx.query_row("SELECT EXISTS(SELECT 1 FROM mail_connections WHERE status!='disconnected') OR EXISTS(SELECT 1 FROM calendar_connections WHERE status!='disconnected')",[],|r|r.get::<_,bool>(0))? {return Err(StoreError::GoogleConnectionsInUse)}
            if identity_changed && tx.query_row("SELECT EXISTS(SELECT 1 FROM mail_connections WHERE status!='disconnected' AND (oauth_client_id IS NULL OR oauth_client_id!=?1)) OR EXISTS(SELECT 1 FROM calendar_connections WHERE status!='disconnected' AND client_id!=?1)",[&client.client_id],|r|r.get::<_,bool>(0))? {return Err(StoreError::GoogleConnectionsInUse)}
            let next=GoogleSetupRecord{revision:expected_revision.checked_add(1).ok_or(StoreError::InvalidInput)?,client_id:client.client_id,redirect_uri:client.redirect_uri};
            let secret=providers::encrypt(&db.provider_key,client.client_secret.expose().as_bytes(),&secret_aad(&next)?)?;
            tx.execute("INSERT INTO google_setup(singleton,revision,client_id,redirect_uri,secret) VALUES(1,?1,?2,?3,?4) ON CONFLICT(singleton) DO UPDATE SET revision=excluded.revision,client_id=excluded.client_id,redirect_uri=excluded.redirect_uri,secret=excluded.secret",params![next.revision,next.client_id,next.redirect_uri,secret])?;
            // Fence in-flight Gmail pages and errors without touching data/checkpoints.
            tx.execute("UPDATE source_sync_runs SET state='unknown',error='lease_invalidated',finished_at=?1 WHERE state='running' AND EXISTS(SELECT 1 FROM source_connections c WHERE c.tenant_id=source_sync_runs.tenant_id AND c.id=source_sync_runs.connection_id AND c.kind='gmail')",[time])?;
            tx.commit()?;Ok(next)
        }).await
    }
    pub async fn google_app(&self, redirect_uri: String) -> Result<Option<GoogleApp>> {
        self.call(move |db| {
            let Some(row) = record(&db.conn)? else {
                return Ok(None);
            };
            if row.redirect_uri != redirect_uri {
                return Err(StoreError::Conflict);
            }
            let cipher: Vec<u8> = db.conn.query_row(
                "SELECT secret FROM google_setup WHERE singleton=1",
                [],
                |r| r.get(0),
            )?;
            let plain = providers::decrypt(&db.provider_key, &cipher, &secret_aad(&row)?)?;
            let client = OAuthClient {
                client_id: row.client_id.clone(),
                redirect_uri: row.redirect_uri.clone(),
                client_secret: SecretString::new(
                    String::from_utf8(plain.to_vec()).map_err(|_| StoreError::Corrupt)?,
                )
                .map_err(|_| StoreError::Corrupt)?,
            };
            Ok(Some(GoogleApp {
                client: Arc::new(client),
                pin: GoogleAppPin {
                    revision: Some(row.revision),
                    client_id: row.client_id,
                    redirect_uri: row.redirect_uri,
                },
            }))
        })
        .await
    }
}
#[cfg(test)]
mod tests;
