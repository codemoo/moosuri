//! A private, serialized browser broker shared by chat tools and human controls.
//! Only human routes accept input; the model can open public pages, observe, or yield.
use moosuri_connectors::{
    browser::{
        BrowserError,
        capsule::{CapsuleReader, CapsuleWorkspace, WorkspaceInput, WorkspaceObservation},
    },
    model::CancellationToken,
};
use moosuri_core::{
    BrowserControl, BrowserFrame, BrowserWorkspaceState as State, BrowserWorkspaceStatus,
    BrowserWorkspaceView, ChatTurn, WebToolRequest,
};
use moosuri_store::{
    BrowserAccess, KnowledgeToolResult, Store, StoreError, ToolReadError, UserContext,
};
use serde_json::json;
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

type Result<T> = std::result::Result<T, StoreError>;
type Key = (String, String);
struct Entry {
    user: UserContext,
    thread: String,
    id: Option<String>,
    worker: Option<CapsuleWorkspace>,
    touched: Instant,
}
#[derive(Clone)]
pub struct BrowserService {
    store: Store,
    reader: Option<Arc<CapsuleReader>>,
    entries: Arc<Mutex<HashMap<Key, Arc<Mutex<Entry>>>>>,
    stop: CancellationToken,
}
impl BrowserService {
    pub(crate) fn new(
        store: Store,
        reader: Option<Arc<CapsuleReader>>,
        stop: CancellationToken,
    ) -> Self {
        Self {
            store,
            reader,
            entries: Arc::new(Mutex::new(HashMap::new())),
            stop,
        }
    }
    pub fn configured(&self) -> bool {
        self.reader.is_some()
    }
    pub async fn status(&self, user: &UserContext, thread: &str) -> Result<BrowserWorkspaceStatus> {
        let mut view = self
            .store
            .browser_workspace(user, thread.to_owned())
            .await?;
        if let Some(v) = &view
            && !terminal(v.state)
        {
            let key = (user.account().id.clone(), thread.to_owned());
            if !self.entries.lock().await.contains_key(&key) {
                view = Some(
                    self.store
                        .end_browser_workspace(
                            user,
                            thread.to_owned(),
                            v.id.clone(),
                            v.revision,
                            State::OutcomeUnknown,
                        )
                        .await?,
                );
            }
        }
        Ok(BrowserWorkspaceStatus {
            configured: self.configured(),
            workspace: view,
        })
    }
    async fn slot(
        &self,
        user: &UserContext,
        thread: &str,
        create: bool,
    ) -> Result<Arc<Mutex<Entry>>> {
        self.store
            .browser_workspace(user, thread.to_owned())
            .await?;
        if self.stop.is_cancelled() {
            return Err(StoreError::Unavailable);
        }
        let mut entries = self.entries.lock().await;
        let key = (user.account().id.clone(), thread.to_owned());
        if let Some(entry) = entries.get(&key) {
            return Ok(entry.clone());
        }
        if !create {
            return Err(StoreError::NotFound);
        }
        if entries.len() >= 16
            || entries
                .keys()
                .filter(|(account, _)| account == &key.0)
                .count()
                >= 2
        {
            return Err(StoreError::Busy);
        }
        let slot = Arc::new(Mutex::new(Entry {
            user: user.clone(),
            thread: thread.to_owned(),
            id: None,
            worker: None,
            touched: Instant::now(),
        }));
        entries.insert(key, slot.clone());
        Ok(slot)
    }
    async fn lock_slot(
        &self,
        slot: Arc<Mutex<Entry>>,
    ) -> Result<tokio::sync::OwnedMutexGuard<Entry>> {
        let entry = slot.clone().lock_owned().await;
        let key = (entry.user.account().id.clone(), entry.thread.clone());
        if !self
            .entries
            .lock()
            .await
            .get(&key)
            .is_some_and(|current| Arc::ptr_eq(current, &slot))
        {
            return Err(StoreError::Conflict);
        }
        Ok(entry)
    }
    async fn check(
        &self,
        user: &UserContext,
        entry: &mut Entry,
        id: &str,
        revision: i64,
        access: BrowserAccess,
    ) -> Result<BrowserWorkspaceView> {
        if self.stop.is_cancelled() {
            return Err(StoreError::Unavailable);
        }
        if entry.id.as_deref() != Some(id) || entry.worker.is_none() {
            return Err(StoreError::Conflict);
        }
        let view = self
            .store
            .check_browser_workspace(user, entry.thread.clone(), id.to_owned(), revision, access)
            .await?;
        entry.touched = Instant::now();
        Ok(view)
    }
    async fn broken(&self, entry: &mut Entry, user: &UserContext) -> Result<()> {
        if let Some(worker) = entry.worker.take() {
            let _ = worker.shutdown().await;
        }
        if let Ok(Some(view)) = self
            .store
            .browser_workspace(user, entry.thread.clone())
            .await
            && entry.id.as_deref() == Some(&view.id)
            && !terminal(view.state)
        {
            self.store
                .end_browser_workspace(
                    user,
                    entry.thread.clone(),
                    view.id,
                    view.revision,
                    State::OutcomeUnknown,
                )
                .await?;
        }
        Ok(())
    }
    async fn outcome<T>(
        &self,
        entry: &mut Entry,
        user: &UserContext,
        result: std::result::Result<T, BrowserError>,
    ) -> Result<T> {
        match result {
            Ok(value) => Ok(value),
            Err(BrowserError::StalePage | BrowserError::InvalidHandle) => Err(StoreError::Conflict),
            Err(BrowserError::InvalidUrl | BrowserError::ForbiddenDestination) => {
                Err(StoreError::InvalidInput)
            }
            Err(BrowserError::Capacity) => Err(StoreError::Busy),
            Err(_) => {
                let _ = self.broken(entry, user).await;
                Err(StoreError::Unavailable)
            }
        }
    }
    async fn open_locked(
        &self,
        entry: &mut Entry,
        user: &UserContext,
        url: &str,
        agent: bool,
    ) -> Result<(BrowserWorkspaceView, WorkspaceObservation)> {
        let reader = self.reader.as_ref().ok_or(StoreError::Unavailable)?;
        let current = self
            .store
            .browser_workspace(user, entry.thread.clone())
            .await?;
        if let Some(view) = current.filter(|v| !terminal(v.state)) {
            if !agent {
                return Err(StoreError::Conflict);
            }
            self.check(
                user,
                entry,
                &view.id,
                view.revision,
                BrowserAccess::AgentNavigate,
            )
            .await?;
            let result = entry
                .worker
                .as_mut()
                .ok_or(StoreError::Conflict)?
                .open(url)
                .await;
            let observation = self.outcome(entry, user, result).await?;
            let updated = self
                .store
                .browser_location(
                    user,
                    view,
                    BrowserAccess::AgentNavigate,
                    observation.page.url.clone(),
                    observation.page.title.clone(),
                )
                .await?;
            return Ok((updated, observation));
        }
        if let Some(worker) = entry.worker.take() {
            worker
                .shutdown()
                .await
                .map_err(|_| StoreError::Unavailable)?;
        }
        let view = self
            .store
            .begin_browser_workspace(
                user,
                entry.thread.clone(),
                reader.deployment_pin().to_owned(),
            )
            .await?;
        entry.id = Some(view.id.clone());
        entry.user = user.clone();
        entry.touched = Instant::now();
        let account = uuid::Uuid::parse_str(&user.account().id).map_err(|_| StoreError::Corrupt)?;
        let started = reader.start_workspace(account, &self.stop).await;
        entry.worker = Some(match started {
            Ok(worker) => worker,
            Err(error) => {
                let mapped = self
                    .outcome::<CapsuleWorkspace>(entry, user, Err(error))
                    .await;
                let _ = self.broken(entry, user).await;
                return Err(mapped.err().unwrap_or(StoreError::Unavailable));
            }
        });
        let result = entry
            .worker
            .as_mut()
            .ok_or(StoreError::Unavailable)?
            .open(url)
            .await;
        let observation = match self.outcome(entry, user, result).await {
            Ok(value) => value,
            Err(error) => {
                let _ = self.broken(entry, user).await;
                return Err(error);
            }
        };
        match self
            .store
            .activate_browser_workspace(
                user,
                entry.thread.clone(),
                view.id,
                observation.page.url.clone(),
                observation.page.title.clone(),
            )
            .await
        {
            Ok(view) => Ok((view, observation)),
            Err(error) => {
                let _ = self.broken(entry, user).await;
                Err(error)
            }
        }
    }
    pub async fn open(
        &self,
        user: &UserContext,
        thread: &str,
        url: &str,
    ) -> Result<BrowserWorkspaceView> {
        let slot = self.slot(user, thread, true).await?;
        let mut entry = self.lock_slot(slot).await?;
        let (view, _) = self.open_locked(&mut entry, user, url, false).await?;
        Ok(view)
    }
    pub async fn takeover(
        &self,
        user: &UserContext,
        thread: &str,
        id: &str,
        revision: i64,
    ) -> Result<BrowserControl> {
        let slot = self.slot(user, thread, false).await?;
        let mut entry = self.lock_slot(slot).await?;
        // Serialize against any already-running navigation. Stale queued operations recheck below.
        self.check(user, &mut entry, id, revision, BrowserAccess::Takeover)
            .await?;
        self.store
            .take_browser_control(user, thread.to_owned(), id.to_owned(), revision)
            .await
    }
    pub async fn frame(
        &self,
        user: &UserContext,
        thread: &str,
        id: &str,
        revision: i64,
        control: Option<String>,
    ) -> Result<BrowserFrame> {
        let slot = self.slot(user, thread, false).await?;
        let mut entry = self.lock_slot(slot).await?;
        let access = control.map_or(BrowserAccess::Watch, BrowserAccess::Human);
        let view = self
            .check(user, &mut entry, id, revision, access.clone())
            .await?;
        let result = entry
            .worker
            .as_mut()
            .ok_or(StoreError::Unavailable)?
            .frame()
            .await;
        let frame = self.outcome(&mut entry, user, result).await?;
        self.store
            .browser_location(
                user,
                view.clone(),
                access,
                frame.url.clone(),
                view.title.unwrap_or_default(),
            )
            .await?;
        Ok(BrowserFrame {
            workspace_id: id.to_owned(),
            revision,
            frame_token: frame.frame_token,
            png_base64: frame.png_base64,
            width: frame.width,
            height: frame.height,
            url: frame.url,
        })
    }
    #[allow(clippy::too_many_arguments)]
    pub async fn input(
        &self,
        user: &UserContext,
        thread: &str,
        id: &str,
        revision: i64,
        control: String,
        frame: &str,
        input: WorkspaceInput,
    ) -> Result<()> {
        let slot = self.slot(user, thread, false).await?;
        let mut entry = self.lock_slot(slot).await?;
        let access = BrowserAccess::Human(control);
        self.check(user, &mut entry, id, revision, access.clone())
            .await?;
        let result = entry
            .worker
            .as_mut()
            .ok_or(StoreError::Unavailable)?
            .input(frame, input)
            .await;
        self.outcome(&mut entry, user, result).await?;
        self.check(user, &mut entry, id, revision, access).await?;
        Ok(())
    }
    pub async fn navigate(
        &self,
        user: &UserContext,
        thread: &str,
        id: &str,
        revision: i64,
        control: String,
        url: &str,
    ) -> Result<BrowserWorkspaceView> {
        let slot = self.slot(user, thread, false).await?;
        let mut entry = self.lock_slot(slot).await?;
        let access = BrowserAccess::Human(control);
        let view = self
            .check(user, &mut entry, id, revision, access.clone())
            .await?;
        // The connector's navigation observation is discarded during human control.
        let result = entry
            .worker
            .as_mut()
            .ok_or(StoreError::Unavailable)?
            .open(url)
            .await;
        let observation = self.outcome(&mut entry, user, result).await?;
        self.store
            .browser_location(
                user,
                view,
                access,
                observation.page.url,
                observation.page.title,
            )
            .await
    }
    pub async fn resume(
        &self,
        user: &UserContext,
        thread: &str,
        id: &str,
        revision: i64,
        control: String,
    ) -> Result<ChatTurn> {
        let slot = self.slot(user, thread, false).await?;
        let mut entry = self.lock_slot(slot).await?;
        if entry.id.as_deref() != Some(id) || entry.worker.is_none() {
            return Err(StoreError::Conflict);
        }
        let turn = self
            .store
            .resume_browser(user, thread.to_owned(), id.to_owned(), revision, control)
            .await?;
        entry.touched = Instant::now();
        Ok(turn)
    }
    pub async fn close(
        &self,
        user: &UserContext,
        thread: &str,
        id: &str,
        revision: i64,
    ) -> Result<BrowserWorkspaceView> {
        let slot = self.slot(user, thread, false).await?;
        let mut entry = self.lock_slot(slot).await?;
        let mut view = self
            .store
            .end_browser_workspace(
                user,
                thread.to_owned(),
                id.to_owned(),
                revision,
                State::Closed,
            )
            .await?;
        if entry.id.as_deref() == Some(id)
            && let Some(worker) = entry.worker.take()
            && worker.shutdown().await.is_err()
        {
            view = self
                .store
                .end_browser_workspace(
                    user,
                    thread.to_owned(),
                    id.to_owned(),
                    view.revision,
                    State::OutcomeUnknown,
                )
                .await?;
        }
        Ok(view)
    }
    pub(crate) async fn execute(
        &self,
        user: &UserContext,
        thread: &str,
        request: &WebToolRequest,
    ) -> KnowledgeToolResult {
        let result = self.model_operation(user, thread, request).await;
        match result {
            Ok(value) => KnowledgeToolResult::External(value),
            Err(StoreError::Conflict) => KnowledgeToolResult::External(
                json!({"browser_status":"human_control_or_changed","instruction":"The human must complete control before observations resume. Never poll or reopen an authenticated page. Ask for direct control in the browser card when needed, then end the response."}),
            ),
            Err(StoreError::NotFound) => KnowledgeToolResult::External(
                json!({"browser_status":"not_open","instruction":"No live workspace. Open a public page first. A closed/expired workspace is not the old authenticated browser."}),
            ),
            Err(StoreError::InvalidInput) => {
                KnowledgeToolResult::Error(ToolReadError::InvalidInput)
            }
            Err(_) => KnowledgeToolResult::Error(ToolReadError::Unavailable),
        }
    }
    async fn model_operation(
        &self,
        user: &UserContext,
        thread: &str,
        request: &WebToolRequest,
    ) -> Result<serde_json::Value> {
        let slot = self
            .slot(
                user,
                thread,
                matches!(request, WebToolRequest::BrowserOpen { .. }),
            )
            .await?;
        let mut entry = self.lock_slot(slot).await?;
        match request {
            WebToolRequest::BrowserOpen { url } => {
                let (view, observation) = self.open_locked(&mut entry, user, url, true).await?;
                Ok(json!({"browser_page":observation.page,"workspace":view}))
            }
            WebToolRequest::BrowserObserve {} => {
                let view = self
                    .store
                    .browser_workspace(user, thread.to_owned())
                    .await?
                    .ok_or(StoreError::NotFound)?;
                self.check(
                    user,
                    &mut entry,
                    &view.id,
                    view.revision,
                    BrowserAccess::AgentObserve,
                )
                .await?;
                let result = entry
                    .worker
                    .as_mut()
                    .ok_or(StoreError::Unavailable)?
                    .observe()
                    .await;
                let observation = self.outcome(&mut entry, user, result).await?;
                let view = self
                    .store
                    .browser_location(
                        user,
                        view,
                        BrowserAccess::AgentObserve,
                        observation.page.url.clone(),
                        observation.page.title.clone(),
                    )
                    .await?;
                Ok(json!({"browser_page":observation.page,"workspace":view}))
            }
            WebToolRequest::BrowserHandoff { reason } => {
                let view = self
                    .store
                    .browser_workspace(user, thread.to_owned())
                    .await?
                    .ok_or(StoreError::NotFound)?;
                self.check(
                    user,
                    &mut entry,
                    &view.id,
                    view.revision,
                    BrowserAccess::AgentObserve,
                )
                .await?;
                let view = self
                    .store
                    .browser_handoff(user, view, reason.clone())
                    .await?;
                Ok(
                    json!({"browser_status":"waiting_for_user","workspace":view,"instruction":"The direct-control card is ready. Explain the one needed action, then finish. Completion resumes this conversation and browser; do not poll."}),
                )
            }
            _ => Err(StoreError::InvalidInput),
        }
    }
    pub(crate) async fn maintenance(&self) {
        let mut tick = tokio::time::interval(Duration::from_secs(2));
        loop {
            tokio::select! {biased;_=self.stop.cancelled()=>break,_=tick.tick()=>{}}
            let entries = self
                .entries
                .lock()
                .await
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect::<Vec<_>>();
            for (key, slot) in entries {
                let Ok(mut entry) = slot.try_lock() else {
                    continue;
                };
                let view = self
                    .store
                    .browser_workspace(&entry.user, entry.thread.clone())
                    .await;
                let lost = entry
                    .worker
                    .as_ref()
                    .is_none_or(CapsuleWorkspace::is_closed);
                let close = lost
                    || entry.touched.elapsed() >= Duration::from_secs(1800)
                    || view.as_ref().is_err()
                    || view
                        .as_ref()
                        .is_ok_and(|v| v.as_ref().is_none_or(|v| terminal(v.state)));
                if close {
                    if let Ok(Some(view)) = view
                        && !terminal(view.state)
                    {
                        let _ = self
                            .store
                            .end_browser_workspace(
                                &entry.user,
                                entry.thread.clone(),
                                view.id,
                                view.revision,
                                if lost {
                                    State::OutcomeUnknown
                                } else {
                                    State::Expired
                                },
                            )
                            .await;
                    }
                    if let Some(worker) = entry.worker.take()
                        && worker.shutdown().await.is_err()
                        && let Ok(Some(view)) = self
                            .store
                            .browser_workspace(&entry.user, entry.thread.clone())
                            .await
                    {
                        let _ = self
                            .store
                            .end_browser_workspace(
                                &entry.user,
                                entry.thread.clone(),
                                view.id,
                                view.revision,
                                State::OutcomeUnknown,
                            )
                            .await;
                    }
                    self.entries.lock().await.remove(&key);
                }
            }
        }
        let slots = self
            .entries
            .lock()
            .await
            .drain()
            .map(|(_, v)| v)
            .collect::<Vec<_>>();
        let mut tasks = tokio::task::JoinSet::new();
        for slot in slots {
            tasks.spawn(async move {
                let mut entry = slot.lock().await;
                if let Some(worker) = entry.worker.take() {
                    let _ = worker.shutdown().await;
                }
            });
        }
        while tasks.join_next().await.is_some() {}
    }
}
fn terminal(state: State) -> bool {
    matches!(
        state,
        State::Closed | State::Expired | State::OutcomeUnknown
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn queued_caller_cannot_start_worker_on_an_entry_removed_by_maintenance() {
        let _guard = crate::tests::TEST_LOCK.lock().await;
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        store
            .bootstrap(
                "Synthetic".into(),
                "registry@example.test".into(),
                "Synthetic-registry-password".into(),
                false,
            )
            .await
            .unwrap();
        let user = store
            .login(
                "registry@example.test".into(),
                "Synthetic-registry-password".into(),
            )
            .await
            .unwrap()
            .user;
        let thread = store
            .create_thread(&user, "Registry race".into())
            .await
            .unwrap()
            .id;
        let service = BrowserService::new(store, None, CancellationToken::new());
        let slot = service.slot(&user, &thread, true).await.unwrap();
        let owned = slot.lock().await;
        let queued_service = service.clone();
        let queued_slot = slot.clone();
        let queued =
            tokio::spawn(async move { queued_service.lock_slot(queued_slot).await.map(|_| ()) });
        tokio::task::yield_now().await;
        service
            .entries
            .lock()
            .await
            .remove(&(user.account().id.clone(), thread.clone()));
        drop(owned);
        assert!(matches!(queued.await.unwrap(), Err(StoreError::Conflict)));
        assert!(service.entries.lock().await.is_empty());
        let fresh = service.slot(&user, &thread, true).await.unwrap();
        assert!(!Arc::ptr_eq(&slot, &fresh));
        assert!(service.lock_slot(fresh).await.is_ok());
    }
}
