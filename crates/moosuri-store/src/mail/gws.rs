//! Runtime-only owner observation for the separately configured GWS helper.
use super::*;
impl Store {
    pub async fn mail_run_owner(&self, lease: &crate::RunLease) -> Result<UserContext> {
        let lease = lease.clone();
        self.call(move |db| {
            let run = crate::chat::guard(&db.conn, &db.provider_key, &lease)?;
            crate::chat::authority(&db.conn, &run)
        })
        .await
    }
}

/// Transaction-local readiness check for owner-bound resumptions.
pub(crate) fn gws_inbox_confirmed_in(
    conn: &Connection,
    user: &UserContext,
    fingerprint: &[u8; 32],
) -> Result<bool> {
    validate(conn, user, true, false)?;
    Ok(conn.query_row("SELECT EXISTS(SELECT 1 FROM gws_inbox_selections WHERE tenant_id=?1 AND account_id=?2 AND auth_epoch=?3 AND fingerprint=?4 AND confirmed=1)", params![user.tenant.0,user.account.id,user.epoch,fingerprint.as_slice()], |r| r.get(0))?)
}
fn observation(conn: &Connection, user: &UserContext) -> Result<Vec<u8>> {
    type InboxSelection = (i64, Vec<u8>, bool, Option<Vec<u8>>, Option<i64>);
    validate(conn, user, true, false)?;
    let row: Option<InboxSelection> = conn.query_row("SELECT auth_epoch,fingerprint,confirmed,challenge_digest,challenge_expires_at FROM gws_inbox_selections WHERE tenant_id=?1 AND account_id=?2",params![user.tenant.0,user.account.id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional()?;
    Ok(Sha256::digest(serde_json::to_vec(&row).map_err(|_| StoreError::Corrupt)?).to_vec())
}
impl Store {
    pub async fn gws_inbox_confirmed(
        &self,
        user: &UserContext,
        fingerprint: [u8; 32],
    ) -> Result<bool> {
        let user = user.clone();
        self.call(move |db| gws_inbox_confirmed_in(&db.conn, &user, &fingerprint))
            .await
    }
    pub async fn gws_inbox_observation(&self, user: &UserContext) -> Result<Vec<u8>> {
        let user = user.clone();
        self.call(move |db| observation(&db.conn, &user)).await
    }
    pub async fn gws_inbox_challenge(
        &self,
        user: &UserContext,
        fingerprint: [u8; 32],
    ) -> Result<Option<String>> {
        let observed = self.gws_inbox_observation(user).await?;
        self.gws_inbox_challenge_observed(user, fingerprint, observed)
            .await
    }
    pub async fn gws_inbox_challenge_observed(
        &self,
        user: &UserContext,
        fingerprint: [u8; 32],
        observed: Vec<u8>,
    ) -> Result<Option<String>> {
        let user = user.clone();
        self.call(move |db| {
            validate(&db.conn,&user,true,false)?;
            if gws_inbox_confirmed_in(&db.conn,&user,&fingerprint)? {return Ok(None);}
            if observation(&db.conn,&user)?!=observed {return Err(StoreError::Conflict);}
            let challenge=random()?;
            db.conn.execute("INSERT INTO gws_inbox_selections(tenant_id,account_id,auth_epoch,fingerprint,confirmed,challenge_digest,challenge_expires_at) VALUES(?1,?2,?3,?4,0,?5,?6) ON CONFLICT(tenant_id,account_id) DO UPDATE SET auth_epoch=excluded.auth_epoch,fingerprint=excluded.fingerprint,confirmed=0,challenge_digest=excluded.challenge_digest,challenge_expires_at=excluded.challenge_expires_at", params![user.tenant.0,user.account.id,user.epoch,fingerprint.as_slice(),hash(&challenge),now()?+600])?;
            Ok(Some(challenge))
        }).await
    }
    pub async fn confirm_gws_inbox(
        &self,
        user: &UserContext,
        fingerprint: [u8; 32],
        challenge: String,
    ) -> Result<()> {
        let user = user.clone();
        self.call(move |db| {
            validate(&db.conn,&user,true,false)?;
            if challenge.len()!=64 || !challenge.bytes().all(|b|b.is_ascii_hexdigit()) {return Err(StoreError::InvalidInput);}
            if db.conn.execute("UPDATE gws_inbox_selections SET confirmed=1,challenge_digest=NULL,challenge_expires_at=NULL WHERE tenant_id=?1 AND account_id=?2 AND auth_epoch=?3 AND fingerprint=?4 AND confirmed=0 AND challenge_digest=?5 AND challenge_expires_at>?6", params![user.tenant.0,user.account.id,user.epoch,fingerprint.as_slice(),hash(&challenge),now()?])? !=1 {return Err(StoreError::Conflict);}
            Ok(())
        }).await
    }
    pub async fn revoke_gws_inbox(&self, user: &UserContext, fingerprint: [u8; 32]) -> Result<()> {
        let user = user.clone();
        self.call(move |db| {
            validate(&db.conn, &user, true, false)?;
            db.conn.execute(
                "DELETE FROM gws_inbox_selections WHERE tenant_id=?1 AND account_id=?2 AND auth_epoch=?3 AND fingerprint=?4",
                params![user.tenant.0, user.account.id,user.epoch,fingerprint.as_slice()],
            )?;
            Ok(())
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{TEST_LOCK, admin, directory, member};
    #[tokio::test]
    async fn gws_confirmation_is_owner_epoch_fingerprint_bound_expiring_and_single_use() {
        let _guard = TEST_LOCK.lock().await;
        let dir = directory();
        let store = Store::open(dir.path()).unwrap();
        let owner = admin(&store).await.user;
        let other = member(&store, &owner, "gws-other@example.test").await.user;
        let pin = [7; 32];
        assert!(!store.gws_inbox_confirmed(&owner, pin).await.unwrap());
        let challenge = store
            .gws_inbox_challenge(&owner, pin)
            .await
            .unwrap()
            .unwrap();
        assert!(
            store
                .confirm_gws_inbox(&other, pin, challenge.clone())
                .await
                .is_err()
        );
        assert!(
            store
                .confirm_gws_inbox(&owner, [8; 32], challenge.clone())
                .await
                .is_err()
        );
        store
            .confirm_gws_inbox(&owner, pin, challenge.clone())
            .await
            .unwrap();
        assert!(store.gws_inbox_confirmed(&owner, pin).await.unwrap());
        let stale = store.gws_inbox_observation(&owner).await.unwrap();
        let next = store
            .gws_inbox_challenge(&owner, [8; 32])
            .await
            .unwrap()
            .unwrap();
        store
            .confirm_gws_inbox(&owner, [8; 32], next)
            .await
            .unwrap();
        assert!(
            store
                .gws_inbox_challenge_observed(&owner, pin, stale)
                .await
                .is_err()
        );
        assert!(store.gws_inbox_confirmed(&owner, [8; 32]).await.unwrap());
        let restore = store
            .gws_inbox_challenge(&owner, pin)
            .await
            .unwrap()
            .unwrap();
        store.confirm_gws_inbox(&owner, pin, restore).await.unwrap();
        assert!(!store.gws_inbox_confirmed(&owner, [8; 32]).await.unwrap());
        assert!(
            store
                .confirm_gws_inbox(&owner, pin, challenge)
                .await
                .is_err()
        );
        assert!(
            store
                .gws_inbox_challenge(&owner, pin)
                .await
                .unwrap()
                .is_none()
        );
        // A stale probe for a different pin cannot revoke the current selection.
        store.revoke_gws_inbox(&owner, [8; 32]).await.unwrap();
        assert!(store.gws_inbox_confirmed(&owner, pin).await.unwrap());
        store.revoke_gws_inbox(&owner, pin).await.unwrap();
        let expired = store
            .gws_inbox_challenge(&owner, pin)
            .await
            .unwrap()
            .unwrap();
        store
            .call(|db| {
                db.conn
                    .execute("UPDATE gws_inbox_selections SET challenge_expires_at=0", [])?;
                Ok(())
            })
            .await
            .unwrap();
        assert!(store.confirm_gws_inbox(&owner, pin, expired).await.is_err());
        let challenge = store
            .gws_inbox_challenge(&owner, pin)
            .await
            .unwrap()
            .unwrap();
        store
            .confirm_gws_inbox(&owner, pin, challenge)
            .await
            .unwrap();
        store.revoke_gws_inbox(&owner, pin).await.unwrap();
        assert!(!store.gws_inbox_confirmed(&owner, pin).await.unwrap());
        let challenge = store
            .gws_inbox_challenge(&owner, pin)
            .await
            .unwrap()
            .unwrap();
        store
            .confirm_gws_inbox(&owner, pin, challenge)
            .await
            .unwrap();
        let changed = store
            .change_password(&owner, "Changed-Synthetic-Password42".into())
            .await
            .unwrap();
        assert!(store.gws_inbox_confirmed(&owner, pin).await.is_err());
        assert!(!store.gws_inbox_confirmed(&changed.user, pin).await.unwrap());
    }
}
