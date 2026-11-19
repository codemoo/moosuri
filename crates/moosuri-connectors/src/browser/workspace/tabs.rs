use super::*;
use capsule::{OpenedTab, WorkspaceTab, WorkspaceTabs};

const MAX_TABS: usize = 4;

impl BrowserWorkspace {
    async fn targets(&mut self) -> Result<Vec<Value>, BrowserError> {
        let response = self
            .session
            .cdp
            .command("Target.getTargets", json!({}), None)
            .await?;
        let infos = response["targetInfos"]
            .as_array()
            .ok_or(BrowserError::Protocol)?;
        if infos.len() > 64 {
            return Err(BrowserError::OutputTooLarge);
        }
        Ok(infos
            .iter()
            .filter(|target| target["type"] == "page")
            .cloned()
            .collect())
    }

    pub async fn start_tabs(&mut self, url: &str) -> Result<OpenedTab, BrowserError> {
        if self.current.is_some() {
            return Err(BrowserError::InvalidHandle);
        }
        // Chrome itself starts with an empty page. Every page subsequently created
        // inside this isolated browser belongs to the workspace, including noopener links.
        self.ignored_targets = bounded!(self, self.targets())?
            .iter()
            .map(|target| string(target, "targetId"))
            .collect::<Result<_, _>>()?;
        let observation = self.open(url).await?;
        Ok(OpenedTab {
            tab_id: self.active()?,
            observation,
        })
    }

    fn active(&self) -> Result<Uuid, BrowserError> {
        self.current
            .as_ref()
            .map(|h| h.page)
            .ok_or(BrowserError::InvalidHandle)
    }

    pub fn require_tab(&self, tab: Uuid) -> Result<(), BrowserError> {
        if self.active()? == tab {
            Ok(())
        } else {
            Err(BrowserError::StalePage)
        }
    }

    pub fn permit_agent_read(&self) -> Result<(), BrowserError> {
        if self.human_interacted {
            Err(BrowserError::ForbiddenDestination)
        } else {
            Ok(())
        }
    }

    pub async fn tabs(&mut self) -> Result<WorkspaceTabs, BrowserError> {
        bounded!(self, self.sync_tabs())
    }

    async fn sync_tabs(&mut self) -> Result<WorkspaceTabs, BrowserError> {
        let infos = self.targets().await?;
        let live: HashSet<&str> = infos
            .iter()
            .filter_map(|t| t["targetId"].as_str())
            .collect();
        self.ignored_targets
            .retain(|target| live.contains(target.as_str()));
        self.session
            .targets
            .retain(|_, t| live.contains(t.target.as_str()));
        let mut tabs = Vec::new();
        for info in &infos {
            let target = string(info, "targetId")?;
            if self.ignored_targets.contains(&target) {
                continue;
            }
            let known = self
                .session
                .targets
                .iter()
                .find(|(_, t)| t.target == target)
                .map(|(id, _)| *id);
            let raw = string(info, "url")?;
            let url = if raw == "about:blank" {
                raw
            } else if let Ok(url) = net::public_url(&raw) {
                url.to_string()
            } else {
                // Internal pages and error documents have no public navigation authority.
                self.session
                    .cdp
                    .command("Target.closeTarget", json!({"targetId":target}), None)
                    .await?;
                self.ignored_targets.insert(target);
                if let Some(id) = known {
                    self.session.targets.remove(&id);
                }
                continue;
            };
            let id = if let Some(id) = known {
                id
            } else if self.session.targets.len() < MAX_TABS {
                self.session.attach_page(target, None).await?.page
            } else {
                self.session
                    .cdp
                    .command("Target.closeTarget", json!({"targetId":target}), None)
                    .await?;
                self.ignored_targets.insert(target);
                continue;
            };
            tabs.push(WorkspaceTab { tab_id: id, url });
        }
        tabs.sort_by_key(|t| t.tab_id);
        if self
            .current
            .as_ref()
            .is_none_or(|h| !tabs.iter().any(|t| t.tab_id == h.page))
        {
            self.current = None;
            self.proof = None;
            self.last_links = None;
            if let Some(tab) = tabs.first() {
                self.activate(tab.tab_id).await?;
            }
        }
        Ok(WorkspaceTabs {
            active_tab: self.current.as_ref().map(|h| h.page),
            tabs,
        })
    }

    async fn activate(&mut self, id: Uuid) -> Result<(), BrowserError> {
        let target = self
            .session
            .targets
            .get(&id)
            .ok_or(BrowserError::StalePage)?;
        self.proof = None;
        self.last_links = None;
        self.session
            .cdp
            .command(
                "Target.activateTarget",
                json!({"targetId":target.target}),
                None,
            )
            .await?;
        self.current = Some(PageHandle {
            page: id,
            worker_generation: self.session.generation,
            navigation_generation: 1,
        });
        Ok(())
    }

    pub async fn switch(&mut self, from: Uuid, to: Uuid) -> Result<WorkspaceTabs, BrowserError> {
        bounded!(self, async {
            self.sync_tabs().await?;
            self.require_tab(from)?;
            self.activate(to).await?;
            self.sync_tabs().await
        })
    }

    pub async fn close_tab(&mut self, id: Uuid) -> Result<WorkspaceTabs, BrowserError> {
        bounded!(self, async {
            self.sync_tabs().await?;
            self.require_tab(id)?;
            self.proof = None;
            self.last_links = None;
            let handle = self.current.take().ok_or(BrowserError::InvalidHandle)?;
            // closeTarget acknowledges initiation. Chrome may still list the page
            // until its renderer exits, especially for a site-opened popup. Never
            // reattach to this target as a new tab during that interval.
            let target = self
                .session
                .targets
                .get(&id)
                .ok_or(BrowserError::InvalidHandle)?
                .target
                .clone();
            self.session.close(&handle).await?;
            self.ignored_targets.insert(target);
            self.sync_tabs().await
        })
    }

    pub async fn follow(&mut self, from: Uuid, raw: &str) -> Result<OpenedTab, BrowserError> {
        let url = net::public_url(raw)?;
        bounded!(self, async {
            self.sync_tabs().await?;
            self.require_tab(from)?;
            self.permit_agent_read()?;
            let (document, links) = self.last_links.take().ok_or(BrowserError::StalePage)?;
            let current = self.refresh().await?;
            if current != document {
                return Err(BrowserError::StalePage);
            }
            // Read navigation only: a named, currently visible, same-origin HTTP link,
            // never dispatch a DOM click or run a page-supplied event handler.
            if url.origin() != net::public_url(&current.url)?.origin()
                || !links.iter().any(|s| s == url.as_str())
            {
                return Err(BrowserError::ForbiddenDestination);
            }
            let fresh = self.observe_inner().await?;
            if self.refresh().await? != document
                || !fresh
                    .page
                    .links
                    .iter()
                    .any(|l| l.url == url.as_str() && !l.text.trim().is_empty())
            {
                return Err(BrowserError::StalePage);
            }
            self.current = Some(self.session.create_page(url.as_str()).await?);
            let observation = self.observe_inner().await?;
            Ok(OpenedTab {
                tab_id: self.active()?,
                observation,
            })
        })
    }
}
