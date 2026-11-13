//! Manual private knowledge. All operations revalidate UserContext inside the writer.
use super::*;
use moosuri_core::*;
use serde::{Serialize, de::DeserializeOwned};
pub(super) mod destructive;
pub(super) mod memories;
pub(super) mod search;
pub(super) mod sources;
#[cfg(test)]
mod tests;
pub(super) fn replay(db: &mut Database) -> Result<()> {
    destructive::replay(db)
}

fn encode<T: Serialize>(value: &T) -> Result<String> {
    serde_json::to_string(value).map_err(|_| StoreError::InvalidInput)
}
fn decode<T: DeserializeOwned>(value: &str) -> Result<T> {
    serde_json::from_str(value).map_err(|_| StoreError::Corrupt)
}
fn hash<T: Serialize>(value: &T) -> Result<[u8; 32]> {
    Ok(Sha256::digest(encode(value)?.as_bytes()).into())
}
fn id(value: &str) -> Result<String> {
    Uuid::parse_str(value)
        .map(|v| v.to_string())
        .map_err(|_| StoreError::InvalidInput)
}
fn label(value: &str, max: usize) -> Result<()> {
    if value.trim().is_empty() || value.len() > max || value.chars().any(char::is_control) {
        Err(StoreError::InvalidInput)
    } else {
        Ok(())
    }
}
fn validity(from: Option<i64>, until: Option<i64>) -> Result<()> {
    if from
        .into_iter()
        .chain(until)
        .any(|v| !(-62_135_596_800..=253_402_300_799).contains(&v))
        || from.zip(until).is_some_and(|(a, b)| a >= b)
    {
        return Err(StoreError::InvalidInput);
    }
    Ok(())
}
fn speaker(value: &Option<String>) -> Result<()> {
    if let Some(v) = value {
        label(v, 200)?;
    }
    Ok(())
}
pub(super) fn generation(conn: &Connection, tenant: &str) -> Result<i64> {
    Ok(conn
        .query_row(
            "SELECT generation FROM knowledge_state WHERE tenant_id=?1",
            [tenant],
            |r| r.get(0),
        )
        .optional()?
        .unwrap_or(0))
}
fn bump(conn: &Connection, tenant: &str) -> Result<i64> {
    conn.execute("INSERT INTO knowledge_state(tenant_id,generation) VALUES (?1,1) ON CONFLICT(tenant_id) DO UPDATE SET generation=generation+1",[tenant])?;
    generation(conn, tenant)
}
fn tenant_key(tenant: &str) -> String {
    format!("t{}", tenant.replace('-', ""))
}
fn normalize(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect()
}
fn remove_index(conn: &Connection, tenant: &str, kind: &str, object: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM knowledge_fts WHERE tenant_key=?1 AND kind=?2 AND object_id=?3",
        params![tenant_key(tenant), kind, object],
    )?;
    Ok(())
}
fn index_text(
    conn: &Connection,
    tenant: &str,
    kind: &str,
    object: &str,
    revision: i64,
    text: &str,
) -> Result<()> {
    remove_index(conn, tenant, kind, object)?;
    conn.execute("INSERT INTO knowledge_fts(tenant_key,kind,object_id,revision,normalized) VALUES (?1,?2,?3,?4,?5)",params![tenant_key(tenant),kind,object,revision,normalize(text)])?;
    Ok(())
}
fn cursor(tenant: &str, scope: &str, generation_value: i64, position: &str) -> Result<String> {
    Ok(encode(&(tenant, scope, generation_value, position))?
        .bytes()
        .map(|b| format!("{b:02x}"))
        .collect())
}
fn uncurs(
    tenant: &str,
    scope: &str,
    generation_value: i64,
    value: Option<String>,
) -> Result<Option<String>> {
    let Some(value) = value else { return Ok(None) };
    if value.len() > 2048 || value.len() % 2 != 0 || !value.is_ascii() {
        return Err(StoreError::InvalidInput);
    }
    let bytes = (0..value.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&value[i..i + 2], 16).map_err(|_| StoreError::InvalidInput))
        .collect::<Result<Vec<_>>>()?;
    let (t, s, g, p): (String, String, i64, String) =
        serde_json::from_slice(&bytes).map_err(|_| StoreError::InvalidInput)?;
    if t != tenant || s != scope || g != generation_value {
        return Err(StoreError::Conflict);
    }
    Ok(Some(p))
}
fn collection(conn: &Connection, tenant: &str, object: &str) -> Result<KnowledgeCollection> {
    conn.query_row("SELECT id,name,scope,revision,cursor,connected,deleted,(SELECT c.id FROM source_connections c WHERE c.tenant_id=knowledge_collections.tenant_id AND c.collection_id=knowledge_collections.id) FROM knowledge_collections WHERE tenant_id=?1 AND id=?2",params![tenant,id(object)?],|r|Ok(KnowledgeCollection{source_connection_id:r.get(7)?,id:r.get(0)?,name:r.get(1)?,scope:r.get(2)?,revision:r.get(3)?,cursor:r.get(4)?,connected:r.get(5)?,deleted:r.get(6)?})).optional()?.ok_or(StoreError::NotFound)
}
impl Store {
    pub async fn create_collection(
        &self,
        user: &UserContext,
        name: String,
        scope: String,
    ) -> Result<KnowledgeCollection> {
        let user = user.clone();
        self.call(move |db| {
            let user = validate(&db.conn, &user, true, false)?;
            label(&name, 200)?;
            label(&scope, 128)?;
            let existing: Option<String> = db
                .conn
                .query_row(
                    "SELECT id FROM knowledge_collections WHERE tenant_id=?1 AND scope=?2",
                    params![user.tenant.0, scope],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(object) = existing {
                let existing = collection(&db.conn, &user.tenant.0, &object)?;
                // The immutable scope also identifies a manual creation retry.
                // A lost ACK must not create duplicates or revive a disconnected set.
                return if existing.name == name.trim() && existing.connected && !existing.deleted {
                    Ok(existing)
                } else {
                    Err(StoreError::Conflict)
                };
            }
            if db.conn.query_row(
                "SELECT count(*) FROM knowledge_collections WHERE tenant_id=?1",
                [&user.tenant.0],
                |r| r.get::<_, i64>(0),
            )? >= 32
            {
                return Err(StoreError::Conflict);
            }
            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            let object = Uuid::new_v4().to_string();
            tx.execute(
                "INSERT INTO knowledge_collections VALUES (?1,?2,?3,?4,1,0,1,0)",
                params![user.tenant.0, object, name.trim(), scope],
            )?;
            bump(&tx, &user.tenant.0)?;
            let result = collection(&tx, &user.tenant.0, &object)?;
            tx.commit()?;
            Ok(result)
        })
        .await
    }
    pub async fn collections(&self, user: &UserContext) -> Result<Vec<KnowledgeCollection>> {
        let user = user.clone();
        self.call(move |db| {
            let user = validate(&db.conn, &user, true, false)?;
            let objects = db
                .conn
                .prepare("SELECT id FROM knowledge_collections WHERE tenant_id=?1 ORDER BY id")?
                .query_map([&user.tenant.0], |r| r.get::<_, String>(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            objects
                .into_iter()
                .map(|v| collection(&db.conn, &user.tenant.0, &v))
                .collect()
        })
        .await
    }
}
