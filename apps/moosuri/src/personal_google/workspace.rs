use super::*;
use local_gws::workspace::WorkspaceData;

pub(super) const STATUS_PATH: &str = "/api/v1/personal/google/workspace";

pub(super) fn action(path: &str) -> Option<(Service, &str)> {
    let (name, action) = path
        .strip_prefix(BASE)?
        .strip_prefix('/')?
        .split_once('/')?;
    Service::WORKSPACE
        .into_iter()
        .find(|s| s.as_str() == name)
        .map(|service| (service, action))
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct WorkspaceConnection {
    service: Service,
    connection_id: Uuid,
    epoch: i64,
    mailbox: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct WorkspaceStatus {
    connections: Vec<WorkspaceConnection>,
}
impl WorkspaceStatus {
    pub(super) fn valid(&self) -> bool {
        let mut seen = std::collections::HashSet::new();
        self.connections.len() <= Service::WORKSPACE.len()
            && self.connections.iter().all(|c| {
                c.service != Service::Gmail
                    && seen.insert(c.service)
                    && !c.connection_id.is_nil()
                    && c.epoch > 0
                    && mailbox(&c.mailbox)
            })
    }
}
pub(super) async fn status(
    State(g): State<Google>,
    headers: HeaderMap,
) -> Result<Response, Failure> {
    let user = g.user(&headers).await?;
    let revision = match &g.backend {
        Backend::Local { config, .. } => {
            Some(registered_client(config).map_err(configuration_failure)?.0)
        }
        _ => None,
    };
    let mut connections = Vec::new();
    for service in Service::WORKSPACE {
        let status = if let Some(revision) = revision {
            g.store
                .member_service_status(&user, g.owner, service, revision)
                .await?
        } else {
            g.store
                .member_service_status_local(&user, g.owner, service)
                .await?
        };
        if let Some(c) = connection(status)? {
            connections.push(WorkspaceConnection {
                service,
                connection_id: c.connection_id,
                epoch: c.epoch,
                mailbox: c.mailbox,
            });
        }
    }
    browser(STATUS_PATH, &WorkspaceStatus { connections })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReadInput {
    connection_id: Uuid,
    epoch: i64,
    resource_id: Option<String>,
}
pub(super) async fn read(
    State(g): State<Google>,
    headers: HeaderMap,
    Json(input): Json<ReadInput>,
) -> Result<Response, Failure> {
    let user = g.user(&headers).await?;
    let resource_id = input.resource_id.as_deref();
    if !local_gws::workspace::resource_is_valid(g.service, resource_id) {
        return Err(StoreError::InvalidInput.into());
    }
    let _serial = g
        .serial
        .try_lock()
        .map_err(|_| Failure(StatusCode::CONFLICT, "conflict"))?;
    let app = g.describe().await?;
    let current = g
        .store
        .member_service_status(&user, g.owner, g.service, app.app_revision)
        .await?;
    let refresh = current.needs_refresh;
    let chosen = Selected {
        connection_id: input.connection_id,
        epoch: input.epoch,
    };
    selected(&chosen, connection(current)?.as_ref())?;
    let binding = read_binding(&g, &user, app.app_revision, &chosen, refresh).await?;
    let page: BrokerReply<Box<serde_json::value::RawValue>> = g
        .call(
            Some(app.app_revision),
            Operation::WorkspaceRead {
                access_token: Secret(&binding.access_token),
                access_binding: &binding.access_binding,
                resource_id,
            },
        )
        .await?;
    let data = WorkspaceData::parse(g.service, page.result.get().as_bytes())
        .map_err(|_| Failure(StatusCode::BAD_GATEWAY, "google_unavailable"))?;
    if data.resource_id() != resource_id {
        return Err(Failure(StatusCode::BAD_GATEWAY, "google_unavailable"));
    }
    g.store.check_member_google_read(&binding).await?;
    browser(&g.path("read"), &data)
}
