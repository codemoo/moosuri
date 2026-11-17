//! One supervised long poller and one bounded sender; the existing agent runs all work.
use super::*;
use moosuri_connectors::telegram::Error;

async fn wait(signals: &Signals, seconds: u64) -> bool {
    tokio::select! {biased;_=signals.stop.cancelled()=>false,_=tokio::time::sleep(Duration::from_secs(seconds))=>true}
}
async fn receive(store: Store, signals: Signals) {
    let mut failures = 0u32;
    loop {
        if signals.stop.is_cancelled() {
            return;
        }
        let bot = match store.telegram_runtime().await {
            Ok(Some(b)) => b,
            _ => {
                if !wait(&signals, 3).await {
                    return;
                }
                continue;
            }
        };
        let result = tokio::select! {biased;_=signals.stop.cancelled()=>return,r=bot.client.updates(bot.offset)=>r};
        let mut error = None;
        match result {
            Ok(mut updates) => {
                updates.sort_by_key(|u| u.update_id);
                for update in updates {
                    if signals.stop.is_cancelled() {
                        return;
                    }
                    match store.ingest_telegram(bot.revision, update).await {
                        Ok(true) => {
                            signals.wake.notify_one();
                            signals.changed()
                        }
                        Ok(false) => (),
                        Err(_) => {
                            error = Some(Error::Unknown);
                            break;
                        }
                    }
                }
            }
            Err(e) => error = Some(e),
        }
        let _ = store.telegram_poll_result(bot.revision, error).await;
        if let Some(e) = error {
            failures = failures.saturating_add(1).min(6);
            let seconds = if let Error::Retry(s) = e {
                u64::from(s)
            } else {
                1 << failures
            };
            if !wait(&signals, seconds).await {
                return;
            }
        } else {
            failures = 0;
            if !wait(&signals, 1).await {
                return;
            }
        }
    }
}
async fn send(store: Store, signals: Signals, origin: Option<String>) {
    let mut last_typing = tokio::time::Instant::now() - Duration::from_secs(5);
    loop {
        if !wait(&signals, 1).await {
            return;
        }
        let bot = match store.telegram_runtime().await {
            Ok(Some(b)) => b,
            _ => continue,
        };
        let typing = store
            .poll_telegram_delivery(bot.revision)
            .await
            .unwrap_or_default();
        if last_typing.elapsed() >= Duration::from_secs(4) {
            last_typing = tokio::time::Instant::now();
            for chat in typing.into_iter().take(8) {
                tokio::select! {biased;_=signals.stop.cancelled()=>return,_=bot.client.typing(chat)=>()}
            }
        }
        let dispatch = {
            let _gate = signals.delivery.read().await;
            if signals.stop.is_cancelled() {
                return;
            }
            store
                .claim_telegram_dispatch(bot.revision, origin.clone())
                .await
        };
        let Ok(Some(dispatch)) = dispatch else {
            continue;
        };
        let result = tokio::select! {biased;_=signals.stop.cancelled()=>Err(Error::Unknown),r=bot.client.send(dispatch.chat,&dispatch.text)=>r};
        // Repeat only the durable local receipt, never the external send.
        for attempt in 0..3 {
            match store.finish_telegram_dispatch(&dispatch, result).await {
                Ok(()) => break,
                Err(StoreError::Busy) if attempt < 2 => {
                    tokio::time::sleep(Duration::from_millis(30)).await
                }
                _ => break,
            }
        }
        if let Err(Error::Retry(s)) = result
            && !wait(&signals, u64::from(s)).await
        {
            return;
        }
    }
}
pub(crate) async fn worker(store: Store, signals: Signals, origin: Option<String>) {
    let mut children = JoinSet::new();
    children.spawn(receive(store.clone(), signals.clone()));
    children.spawn(send(store, signals, origin));
    let _ = children.join_next().await;
    // Scope owns both tasks; a stopped or failed child cannot leave a second poller.
    children.abort_all();
    while children.join_next().await.is_some() {}
}
