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
