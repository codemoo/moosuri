//! Read-only proof that a nonbearer incoming-share origin still belongs to the
//! currently authorized account. Expiry alone is allowed; revocation never is.
use super::*;
const MAX_ORIGIN_SESSIONS: usize = 256;

pub(super) fn draft_scope(digest: &[u8; 32], account: &str, epoch: i64) -> String {
    let mut hash = Sha256::new();
    hash.update(b"moosuri/chat-drafts/scope/v1\0");
    hash.update(digest);
    hash.update((account.len() as u64).to_be_bytes());
    hash.update(account.as_bytes());
    hash.update(epoch.to_be_bytes());
    format!("{:x}", hash.finalize())
}
impl Store {
    /// The input is not a credential. A fresh unrestricted UserContext is always
    /// required, including for administrators; no arbitrary account is accepted.
    pub async fn validate_share_origin(&self, user: &UserContext, origin: String) -> Result<()> {
        if origin.len() != 64
            || !origin
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(StoreError::InvalidInput);
        }
        let user = user.clone();
        self.call(move |db| {
            let current = validate(&db.conn, &user, true, false)?;
            // Retained naturally expired rows are meaningful provenance. Logout
            // deletes a row; password/admin/recovery transitions delete/epoch it.
            // Never accept an old epoch or password-change-only origin.
            let mut statement = db.conn.prepare("SELECT digest,auth_epoch,password_change_only FROM sessions WHERE tenant_id=?1 AND account_id=?2 LIMIT ?3")?;
            let rows = statement.query_map(params![current.tenant.0, current.account.id, (MAX_ORIGIN_SESSIONS + 1) as i64], |r| Ok((r.get::<_, Vec<u8>>(0)?,r.get::<_, i64>(1)?,r.get::<_, bool>(2)?)))?;
            let mut count = 0;
            let mut found = false;
            for row in rows {
                count += 1;
                if count > MAX_ORIGIN_SESSIONS { return Err(StoreError::Forbidden); }
                let (digest, epoch, restricted) = row?;
                if epoch != current.epoch || restricted { continue; }
                let bytes: [u8; 32] = digest.try_into().map_err(|_| StoreError::Corrupt)?;
                found |= draft_scope(&bytes, &current.account.id, current.epoch) == origin;
            }
            if found { Ok(()) } else { Err(StoreError::Forbidden) }
        }).await
    }
}
#[cfg(test)]
mod tests;
