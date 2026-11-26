//! Bounded member discovery and one allocation-bound lifecycle receipt at a time.
use super::*;

#[derive(Default)]
pub(super) struct State {
    after: String,
    pending: Option<(Scope, Reply, Instant)>,
}

fn response_of(reply: &Reply) -> Result<Response, StatusCode> {
    Ok(response(
        StatusCode::OK,
        serde_json::to_vec(reply).map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?,
    ))
}

async fn request(
    g: &Gateway,
    a: &SessionAllocation,
    request: &Request,
) -> Result<Reply, StatusCode> {
    let payload = serde_json::to_vec(request).map_err(|_| StatusCode::BAD_REQUEST)?;
    let upstream = g
        .proxy(a, Method::POST, AUTHORITY_PATH, Some(&payload), None)
        .await;
    recheck(g, a).await?;
    let (status, bytes, _) = upstream?;
    if !status.is_success() {
        return Err(status);
    }
    let reply: Reply =
        serde_json::from_slice(&bytes).map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    if status != StatusCode::OK
        || reply.owner.account.to_string() != a.account_id
        || reply.owner.tenant.to_string() != a.tenant_id
        || !matches(request, &reply)
    {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }
    Ok(reply)
}

pub(super) async fn handle(
    g: &Gateway,
    v: &Enrollment,
    req: &Request,
) -> Result<Response, StatusCode> {
    // Hold admission over upstream awaits. Concurrent Next/Ack cannot replace a
    // receipt or acknowledge an event selected for a different member.
    let mut state = v
        .lifecycle
        .try_lock()
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    // A failed ciphertext application may never send Ack. Durable events remain
    // in the member runtime; expire this routing cache so others can progress.
    if state
        .pending
        .as_ref()
        .is_some_and(|(_, _, expires)| *expires <= Instant::now())
    {
        state.pending = None;
    }
    if let Some((scope, event, _)) = state.pending.as_ref() {
        if let Request::LifecycleAck { event: id } = req {
            if event.operation != Some(*id) {
                return Err(StatusCode::CONFLICT);
            }
        }
        let a = resolve(
            g,
            runtime_registry::SessionDestination::Account(scope.account),
        )
        .await;
        let a = match a {
            Ok(a) if scope_of(&a)? == *scope => a,
            Ok(_) | Err(StatusCode::UNAUTHORIZED) => {
                state.pending = None;
                return Err(StatusCode::UNAUTHORIZED);
            }
            Err(e) => return Err(e),
        };
        if matches!(req, Request::LifecycleNext) {
            // Redelivery uses the identical receipt, including installation/auth epoch.
            return response_of(event);
        }
        let ack = match request(g, &a, req).await {
            Ok(ack) => ack,
            Err(e) => {
                state.pending = None;
                return Err(e);
            }
        };
        if ack.owner != event.owner
            || ack.operation != event.operation
            || ack.digest != event.digest
        {
            return Err(StatusCode::SERVICE_UNAVAILABLE);
        }
        let response = response_of(&ack)?;
        state.pending = None;
        return Ok(response);
    }
    if !matches!(req, Request::LifecycleNext) {
        return Err(StatusCode::CONFLICT);
    }
    let members = if let Some(account) = v.account {
        vec![account]
    } else {
        let root = g.registry.clone();
        let after = state.after.clone();
        let members = tokio::task::spawn_blocking(move || {
            let members = runtime_registry::scheduled_member_page(&root, after.clone())?;
            if members.is_empty() && !after.is_empty() {
                runtime_registry::scheduled_member_page(&root, String::new())
            } else {
                Ok(members)
            }
        })
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?
        .map_err(route_error)?;
        if members.is_empty() {
            state.after.clear();
        }
        members
    };
    let mut failed = false;
    for account in members {
        // Advance even when this member is unavailable: no member can starve the rest.
        state.after = account.to_string();
        let outcome = async {
            let a = resolve(g, runtime_registry::SessionDestination::Account(account)).await?;
            let event = request(g, &a, req).await?;
            Ok::<_, StatusCode>((scope_of(&a)?, event))
        }
        .await;
        match outcome {
            Ok((scope, event)) => {
                let response = response_of(&event)?;
                state.pending = Some((scope, event, Instant::now() + Duration::from_secs(30)));
                return Ok(response);
            }
            Err(e) if v.account.is_some() => return Err(e),
            Err(StatusCode::NOT_FOUND | StatusCode::UNAUTHORIZED) => {}
            Err(_) => failed = true,
        }
    }
    Err(if failed {
        StatusCode::SERVICE_UNAVAILABLE
    } else {
        StatusCode::NOT_FOUND
    })
}
