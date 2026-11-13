//! Five-second DB-only local reminder worker. Idle work performs no model/network calls.
use super::*;
pub(crate) async fn worker(store: Store, signals: Signals) {
    let mut tick = tokio::time::interval(Duration::from_secs(5));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {biased;_=signals.stop.cancelled()=>break,_=tick.tick()=>{}}
        if store.enqueue_scheduled_reminders().await.is_err() {
            continue;
        }
        if store.poll_recurring_missions().await.is_ok() {
            signals.changed();
            signals.wake.notify_one();
        }
        for _ in 0..32 {
            if signals.stop.is_cancelled() {
                return;
            }
            let Ok(Some(lease)) = store.claim_scheduled_reminder().await else {
                break;
            };
            let _gate = tokio::select! {biased;_=signals.stop.cancelled()=>return,gate=signals.delivery.write()=>gate};
            if signals.stop.is_cancelled() {
                return;
            }
            if store.finish_scheduled_reminder(&lease).await.is_ok() {
                signals.changed();
            }
        }
    }
}
