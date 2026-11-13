//! Independent bounded delivery worker. No model calls and no private diagnostics.
use super::*;
use moosuri_connectors::push::{PreparedPush, PushOutcome};
use moosuri_store::push::PushClaim;
use std::future::Future;
trait Transport: Clone + Send + Sync + 'static {
    type Prepared: Send;
    fn prepare(&self, claim: &PushClaim)
    -> impl Future<Output = Result<Self::Prepared, ()>> + Send;
    fn send(
        &self,
        prepared: Self::Prepared,
        stop: &CancellationToken,
    ) -> impl Future<Output = PushOutcome> + Send;
}
#[derive(Clone)]
struct Network;
impl Transport for Network {
    type Prepared = PreparedPush;
    async fn prepare(&self, claim: &PushClaim) -> Result<PreparedPush, ()> {
        PreparedPush::prepare(
            &claim.subscription,
            &claim.vapid,
            &claim.contact,
            &claim.payload,
        )
        .await
        .map_err(|_| ())
    }
    async fn send(&self, prepared: PreparedPush, stop: &CancellationToken) -> PushOutcome {
        prepared.send(stop).await
    }
}
async fn deliver<T: Transport>(store: Store, claim: PushClaim, signals: Signals, transport: T) {
    let prepared = tokio::select! {biased;_=signals.stop.cancelled()=>{let _=store.fail_push_preparation(&claim.lease).await;return},value=transport.prepare(&claim)=>value};
    let Ok(prepared) = prepared else {
        let _ = store.fail_push_preparation(&claim.lease).await;
        return;
    };
    if signals.stop.is_cancelled() || store.commit_push_dispatch(&claim.lease).await.is_err() {
        return;
    }
    let outcome = transport.send(prepared, &signals.stop).await;
    // Only repeat local settlement, never network work after an uncertain ACK.
    for attempt in 0..3 {
        match store.finish_push(&claim.lease, outcome).await {
            Ok(()) => {
                signals.changed();
                break;
            }
            Err(StoreError::Busy) if attempt < 2 => {
                tokio::time::sleep(Duration::from_millis(30)).await
            }
            _ => break,
        }
    }
}
async fn worker_with<T: Transport>(store: Store, signals: Signals, transport: T) {
    let mut tick = tokio::time::interval(Duration::from_secs(5));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut jobs = JoinSet::new();
    loop {
        tokio::select! {biased;_=signals.stop.cancelled()=>break,result=jobs.join_next(),if !jobs.is_empty()=>{let _=result;},_=tick.tick()=>{let _=store.poll_push_queue().await;}}
        while jobs.len() < 2 && !signals.stop.is_cancelled() {
            match store.claim_push().await {
                Ok(Some(claim)) => {
                    jobs.spawn(deliver(
                        store.clone(),
                        claim,
                        signals.clone(),
                        transport.clone(),
                    ));
                }
                _ => break,
            }
        }
    }
    while jobs.join_next().await.is_some() {}
}
pub(crate) async fn worker(store: Store, signals: Signals) {
    worker_with(store, signals, Network).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use moosuri_core::{ScheduleCreate, ScheduleDraft, ScheduleRule};
    use moosuri_store::push::PushEnrollment;
    use std::sync::atomic::{AtomicUsize, Ordering};
    #[derive(Clone)]
    struct Script {
        calls: Arc<AtomicUsize>,
        entered: Arc<Notify>,
        release: Arc<Notify>,
        hold: bool,
    }
    impl Transport for Script {
        type Prepared = ();
        async fn prepare(&self, claim: &PushClaim) -> Result<(), ()> {
            let value: serde_json::Value = serde_json::from_slice(&claim.payload).unwrap();
            assert_eq!(value.as_object().unwrap().len(), 3);
            assert!(value.get("notification_id").is_some() && value.get("binding_id").is_some());
            self.entered.notify_one();
            if self.hold {
                self.release.notified().await;
            }
            Ok(())
        }
        async fn send(&self, _: (), _: &CancellationToken) -> PushOutcome {
            self.calls.fetch_add(1, Ordering::SeqCst);
            PushOutcome::Accepted
        }
    }
    fn signals() -> Signals {
        let (changes, _) = watch::channel(0);
        Signals {
            stop: CancellationToken::new(),
            wake: Arc::new(Notify::new()),
            changes,
            delivery: Arc::new(tokio::sync::RwLock::new(())),
        }
    }
    async fn fixture() -> (tempfile::TempDir, Store, moosuri_store::UserContext, String) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let pass = "Synthetic-push-worker-password";
        store
            .bootstrap(
                "owner".into(),
                "owner@example.test".into(),
                pass.into(),
                false,
            )
            .await
            .unwrap();
        let user = store
            .login("owner@example.test".into(), pass.into())
            .await
            .unwrap()
            .user;
        let device = uuid::Uuid::new_v4().to_string();
        let (_, public) = moosuri_connectors::push::generate_vapid().unwrap();
        store
            .enroll_push(
                &user,
                PushEnrollment {
                    operation_revision: 1,
                    request_id: uuid::Uuid::new_v4().to_string(),
                    device_id: device.clone(),
                    subscription: moosuri_connectors::push::PushSubscription {
                        endpoint: "https://fcm.googleapis.com/fcm/send/synthetic-worker-only"
                            .into(),
                        p256dh: public,
                        auth: "_ordMnz7uTCmrpBTeUV4Bw".into(), // gitleaks:allow -- synthetic Web Push fixture, not a live subscription
                    },
                },
                "https://push-owner.example.test".into(),
            )
            .await
            .unwrap();
        let at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
            + 60;
        store
            .create_schedule(
                &user,
                ScheduleCreate {
                    request_id: uuid::Uuid::new_v4().to_string(),
                    draft: ScheduleDraft {
                        title: "Synthetic private title".into(),
                        message: "Synthetic private body".into(),
                        rule: ScheduleRule::Once { at },
                        valid_until: None,
                    },
                },
            )
            .await
            .unwrap();
        store.enqueue_scheduled_reminders_at(at).await.unwrap();
        let lease = store
            .claim_scheduled_reminder_at(at)
            .await
            .unwrap()
            .unwrap();
        store
            .finish_scheduled_reminder_at(&lease, at)
            .await
            .unwrap();
        (dir, store, user, device)
    }
    #[tokio::test]
    async fn supervised_queue_delivers_once_through_scripted_transport_and_keeps_inbox() {
        let _guard = crate::tests::TEST_LOCK.lock().await;
        let (_dir, store, user, device) = fixture().await;
        let script = Script {
            calls: Arc::new(AtomicUsize::new(0)),
            entered: Arc::new(Notify::new()),
            release: Arc::new(Notify::new()),
            hold: false,
        };
        let signals = signals();
        let task = tokio::spawn(worker_with(store.clone(), signals.clone(), script.clone()));
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let cfg = store
                    .push_configuration(
                        &user,
                        device.clone(),
                        "https://push-owner.example.test".into(),
                    )
                    .await
                    .unwrap();
                if cfg.device.unwrap().last_outcome.as_deref() == Some("accepted") {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        signals.stop.cancel();
        task.await.unwrap();
        assert_eq!(script.calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            store
                .notifications(&user, None, false)
                .await
                .unwrap()
                .items
                .len(),
            1
        );
    }
    #[tokio::test]
    async fn logout_during_preparation_prevents_actual_send() {
        let _guard = crate::tests::TEST_LOCK.lock().await;
        let (_dir, store, user, _device) = fixture().await;
        let script = Script {
            calls: Arc::new(AtomicUsize::new(0)),
            entered: Arc::new(Notify::new()),
            release: Arc::new(Notify::new()),
            hold: true,
        };
        let signals = signals();
        let task = tokio::spawn(worker_with(store.clone(), signals.clone(), script.clone()));
        tokio::time::timeout(Duration::from_secs(3), script.entered.notified())
            .await
            .unwrap();
        store.logout(&user).await.unwrap();
        script.release.notify_one();
        tokio::time::sleep(Duration::from_millis(50)).await;
        signals.stop.cancel();
        task.await.unwrap();
        assert_eq!(script.calls.load(Ordering::SeqCst), 0);
    }
}
