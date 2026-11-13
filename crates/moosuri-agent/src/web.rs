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
    workspaces: Option<crate::browser::BrowserService>,
    client: Option<std::sync::Arc<WebClient>>,
    browser: Option<std::sync::Arc<moosuri_connectors::browser::capsule::CapsuleReader>>,
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
    pub(crate) fn reader(
        &self,
    ) -> Option<std::sync::Arc<moosuri_connectors::browser::capsule::CapsuleReader>> {
        self.browser.clone()
    }
    pub(crate) fn set_workspaces(&mut self, service: crate::browser::BrowserService) {
        self.workspaces = Some(service);
    }
    pub(crate) fn workspaces(&self) -> Option<crate::browser::BrowserService> {
        self.workspaces.clone()
    }
    pub(crate) fn new(client: Option<std::sync::Arc<WebClient>>) -> Self {
        Self {
            client,
            browser: None,
            workspaces: None,
            #[cfg(test)]
            fixture: None,
        }
    }
    pub(crate) fn with_browser(
        mut self,
        browser: Option<std::sync::Arc<moosuri_connectors::browser::capsule::CapsuleReader>>,
    ) -> Self {
        self.browser = browser;
        self
    }
    #[cfg(test)]
    pub(crate) fn fixture(fixture: std::sync::Arc<Fixture>) -> Self {
        Self {
            client: None,
            browser: None,
            workspaces: None,
            fixture: Some(fixture),
        }
    }
    pub(crate) async fn execute(
        &self,
        request: &WebToolRequest,
        user: &moosuri_store::UserContext,
        thread: &str,
        browser_pin: Option<&str>,
        cancel: &CancellationToken,
    ) -> KnowledgeToolResult {
        #[cfg(test)]
        if let Some(fixture) = &self.fixture {
            return fixture(request.clone(), cancel.clone()).await;
        }
        if matches!(
            request,
            WebToolRequest::BrowserOpen { .. }
                | WebToolRequest::BrowserObserve {}
                | WebToolRequest::BrowserHandoff { .. }
        ) {
            if self
                .browser
                .as_ref()
                .is_none_or(|reader| Some(reader.deployment_pin()) != browser_pin)
            {
                return KnowledgeToolResult::Error(ToolReadError::Unavailable);
            }
            return match &self.workspaces {
                Some(service) => service.execute(user, thread, request).await,
                None => KnowledgeToolResult::Error(ToolReadError::Unavailable),
            };
        }
        if let WebToolRequest::BrowserRead { url } = request {
            let Some(reader) = self
                .browser
                .as_ref()
                .filter(|reader| Some(reader.deployment_pin()) == browser_pin)
            else {
                return KnowledgeToolResult::Error(ToolReadError::Unavailable);
            };
            let Ok(account) = uuid::Uuid::parse_str(&user.account().id) else {
                return KnowledgeToolResult::Error(ToolReadError::Unavailable);
            };
            return match reader.read(account, url, cancel).await {
                Ok(page) => KnowledgeToolResult::External(json!({"browser_page":page})),
                Err(
                    moosuri_connectors::browser::BrowserError::InvalidUrl
                    | moosuri_connectors::browser::BrowserError::ForbiddenDestination,
                ) => KnowledgeToolResult::Error(ToolReadError::InvalidInput),
                Err(moosuri_connectors::browser::BrowserError::OutputTooLarge) => {
                    KnowledgeToolResult::Error(ToolReadError::TooLargeNarrowRead)
                }
                Err(_) => KnowledgeToolResult::Error(ToolReadError::Unavailable),
            };
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
        WebToolRequest::BrowserRead { .. }
        | WebToolRequest::BrowserOpen { .. }
        | WebToolRequest::BrowserObserve {}
        | WebToolRequest::BrowserHandoff { .. } => {
            return KnowledgeToolResult::Error(ToolReadError::Unavailable);
        }
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
