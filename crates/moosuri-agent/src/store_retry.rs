//! Bounded local Store pressure handling, never model dispatch or external IO.
use moosuri_store::StoreError;
use std::{future::Future, time::Duration};

enum Policy {
    Local,
    Authority,
}

/// Retry the identical local operation only when Store reports Busy.
/// Unavailable may mean a lost acknowledgment: never turn it into a replay.
pub(crate) async fn local<T, F, Fut>(operation: F) -> Result<T, StoreError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, StoreError>>,
{
    retry(Policy::Local, operation).await
}

/// Rechecking the same run lease may retry transient unavailability as well.
/// Store can durably settle an expired/revoked run while checking; it never
/// dispatches work. A revoked lease or exhausted budget is returned immediately.
pub(crate) async fn authority<F, Fut>(check: F) -> Result<(), StoreError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<(), StoreError>>,
{
    retry(Policy::Authority, check).await
}

async fn retry<T, F, Fut>(policy: Policy, mut operation: F) -> Result<T, StoreError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, StoreError>>,
{
    for attempt in 0..5 {
        let result = operation().await;
        let transient = matches!(&result, Err(StoreError::Busy))
            || matches!(policy, Policy::Authority)
                && matches!(&result, Err(StoreError::Unavailable));
        if !transient || attempt == 4 {
            return result;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    unreachable!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::ready;

    #[tokio::test(start_paused = true)]
    async fn transient_store_pressure_recovers_without_revoking_authority() {
        let mut attempts = 0;
        let result = authority(|| {
            attempts += 1;
            ready(if attempts < 3 {
                Err(StoreError::Busy)
            } else {
                Ok(())
            })
        })
        .await;
        assert!(result.is_ok());
        assert_eq!(attempts, 3);
        attempts = 0;
        let result = authority(|| {
            attempts += 1;
            ready(Err(StoreError::Unavailable))
        })
        .await;
        assert!(matches!(result, Err(StoreError::Unavailable)));
        assert_eq!(attempts, 5);
        attempts = 0;
        let result = authority(|| {
            attempts += 1;
            ready(Err(StoreError::Unauthorized))
        })
        .await;
        assert_eq!(result, Err(StoreError::Unauthorized));
        assert_eq!(attempts, 1);
    }

    #[tokio::test(start_paused = true)]
    async fn local_receipts_retry_busy_but_never_replay_unknown_or_revoked_outcomes() {
        let mut attempts = 0;
        let mut committed = 0;
        let result = local(|| {
            attempts += 1;
            ready(if attempts < 3 {
                Err(StoreError::Busy)
            } else {
                committed += 1;
                Ok("same-receipt")
            })
        })
        .await;
        assert_eq!(result, Ok("same-receipt"));
        assert_eq!((attempts, committed), (3, 1));

        // Model a local commit whose reply is lost: a repeated closure could
        // duplicate the write. The policy must preserve the unknown result.
        committed = 0;
        let result = local(|| {
            committed += 1;
            ready(if committed == 1 {
                Err(StoreError::Unavailable)
            } else {
                Ok(())
            })
        })
        .await;
        assert_eq!(result, Err(StoreError::Unavailable));
        assert_eq!(committed, 1);

        attempts = 0;
        let result = local(|| {
            attempts += 1;
            ready(if attempts == 1 {
                Err(StoreError::Conflict)
            } else {
                Ok(())
            })
        })
        .await;
        assert_eq!(result, Err(StoreError::Conflict));
        assert_eq!(attempts, 1);

        attempts = 0;
        let result: Result<(), _> = local(|| {
            attempts += 1;
            ready(Err(StoreError::Busy))
        })
        .await;
        assert_eq!(result, Err(StoreError::Busy));
        assert_eq!(attempts, 5);
    }
}
