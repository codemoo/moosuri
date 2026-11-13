//! Metadata-only admin operations, with final commits behind the delivery fence.
use super::*;
use moosuri_core::{
    AccountManagementInput, AccountManagementReceipt, AccountSummary, ManagedAccount,
};

#[derive(Serialize)]
pub(super) struct ManagedUsersResponse {
    pub users: Vec<AccountSummary>,
    pub management: Vec<ManagedAccount>,
}
#[derive(Serialize)]
struct ManagementResponse {
    receipt: AccountManagementReceipt,
    temporary_password: Option<String>,
}
pub(super) async fn manage(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<AccountManagementInput>,
) -> HttpResult {
    let user = current(&state, &headers).await?;
    let prepared = state
        .store
        .prepare_account_management(&user, id, input)
        .await?;
    let _gate = state.delivery.write().await;
    let result = state.store.apply_account_management(prepared).await?;
    state.runtime.wake();
    Ok(Json(ManagementResponse {
        receipt: result.receipt,
        temporary_password: result.temporary_password.map(|p| p.to_string()),
    })
    .into_response())
}
