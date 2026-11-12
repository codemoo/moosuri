//! Public research IO runs outside the SQLite actor. It grants no action authority.
use moosuri_connectors::{
    model::CancellationToken,
    web::{WebClient, WebError},
};
use moosuri_core::WebToolRequest;
use moosuri_store::{KnowledgeToolResult, ToolReadError};
use serde_json::json;

#[derive(Clone, Default)]
pub(crate) struct Service {
    client: Option<std::sync::Arc<WebClient>>,
    #[cfg(test)]
    fixture: Option<std::sync::Arc<Fixture>>,
}
#[cfg(test)]
type Fixture = dyn Fn(
        WebToolRequest,
        CancellationToken,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = KnowledgeToolResult> + Send>>
    + Send
    + Sync;
impl Service {
    pub(crate) fn new(client: Option<std::sync::Arc<WebClient>>) -> Self {
        Self {
            client,
            #[cfg(test)]
            fixture: None,
        }
    }
    #[cfg(test)]
    pub(crate) fn fixture(fixture: std::sync::Arc<Fixture>) -> Self {
        Self {
            client: None,
            fixture: Some(fixture),
        }
    }
    pub(crate) async fn execute(
        &self,
        request: &WebToolRequest,
        cancel: &CancellationToken,
    ) -> KnowledgeToolResult {
        #[cfg(test)]
        if let Some(fixture) = &self.fixture {
            return fixture(request.clone(), cancel.clone()).await;
        }
        execute(self.client.as_deref(), request, cancel).await
    }
}
async fn execute(
    client: Option<&WebClient>,
    request: &WebToolRequest,
    cancel: &CancellationToken,
) -> KnowledgeToolResult {
    let Some(client) = client else {
        return KnowledgeToolResult::Error(ToolReadError::Unavailable);
    };
    let result = match request {
        WebToolRequest::Read { url } => client
            .read(url, cancel)
            .await
            .map(|page| json!({"web_page": page})),
        WebToolRequest::Search { query } => client
            .search(query, cancel)
            .await
            .map(|search| json!({"web_search": search})),
    };
    match result {
        Ok(result) => KnowledgeToolResult::External(result),
        Err(WebError::InvalidUrl | WebError::ForbiddenDestination) => {
            KnowledgeToolResult::Error(ToolReadError::InvalidInput)
        }
        Err(WebError::TooLarge) => KnowledgeToolResult::Error(ToolReadError::TooLargeNarrowRead),
        Err(_) => KnowledgeToolResult::Error(ToolReadError::Unavailable),
    }
}
