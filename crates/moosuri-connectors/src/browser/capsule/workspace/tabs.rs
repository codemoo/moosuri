use super::*;

/// Fixed operations for the authenticated member relay. Frame and input operations
/// belong exclusively to the human UI. No generic worker command is accepted.
#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum TabCommand {
    Open {
        url: String,
    },
    Tabs,
    Frame {
        tab_id: Uuid,
    },
    Observe {
        tab_id: Uuid,
    },
    Selected {
        tab_id: Uuid,
        frame_token: String,
    },
    Input {
        tab_id: Uuid,
        frame_token: String,
        input: WorkspaceInput,
    },
    Follow {
        tab_id: Uuid,
        url: String,
    },
    Switch {
        tab_id: Uuid,
        to_tab_id: Uuid,
    },
    CloseTab {
        tab_id: Uuid,
    },
    Stop,
}

fn validated_tabs(value: Value) -> Result<Value, BrowserError> {
    let tabs: WorkspaceTabs = serde_json::from_value(value).map_err(|_| BrowserError::Protocol)?;
    if tabs.tabs.len() > 4 || tabs.active_tab.is_some() == tabs.tabs.is_empty() {
        return Err(BrowserError::Protocol);
    }
    let mut seen = std::collections::HashSet::new();
    for t in &tabs.tabs {
        if t.tab_id.is_nil() || !seen.insert(t.tab_id) {
            return Err(BrowserError::Protocol);
        }
        if t.url != "about:blank" {
            public_url(&t.url)?;
        }
    }
    if tabs.active_tab.is_some_and(|id| !seen.contains(&id)) {
        return Err(BrowserError::Protocol);
    }
    Ok(json!(tabs))
}

fn validate_opened(value: Value) -> Result<Value, BrowserError> {
    let tab: OpenedTab = serde_json::from_value(value).map_err(|_| BrowserError::Protocol)?;
    if tab.tab_id.is_nil() {
        return Err(BrowserError::Protocol);
    }
    observation(json!(tab.observation))?;
    Ok(json!(tab))
}

fn selected_observation(value: Value) -> Result<Value, BrowserError> {
    let mut obs = observation(value)?;
    let mut url = public_url(&obs.page.url)?;
    url.set_query(None);
    url.set_fragment(None);
    obs.page.url = url.to_string();
    obs.page.links.clear();
    Ok(json!(obs))
}

impl CapsuleWorkspace {
    pub async fn tab_command(&mut self, cmd: TabCommand) -> Result<Value, BrowserError> {
        let (body, limit, check): (_, _, fn(Value) -> Result<Value, BrowserError>) = match cmd {
            TabCommand::Open { url } => (
                json!({"op":"workspace_start_tabs","url":public_url(&url)?.as_str()}),
                MAX_RESPONSE,
                validate_opened,
            ),
            TabCommand::Follow { tab_id, url } => (
                json!({"op":"workspace_follow","tab_id":tab_id,"url":public_url(&url)?.as_str()}),
                MAX_RESPONSE,
                validate_opened,
            ),
            TabCommand::Tabs => (json!({"op":"workspace_tabs"}), 16384, validated_tabs),
            TabCommand::Switch { tab_id, to_tab_id } => (
                json!({"op":"workspace_switch","tab_id":tab_id,"to_tab_id":to_tab_id}),
                16384,
                validated_tabs,
            ),
            TabCommand::CloseTab { tab_id } => (
                json!({"op":"workspace_close_tab","tab_id":tab_id}),
                16384,
                validated_tabs,
            ),
            TabCommand::Frame { tab_id } => (
                json!({"op":"workspace_tab_frame","tab_id":tab_id}),
                MAX_FRAME_RESPONSE,
                |v| frame(v).map(|f| json!(f)),
            ),
            TabCommand::Observe { tab_id } => (
                json!({"op":"workspace_tab_observe","tab_id":tab_id}),
                MAX_RESPONSE,
                |v| observation(v).map(|o| json!(o)),
            ),
            TabCommand::Selected {
                tab_id,
                frame_token,
            } => {
                if !hex64(&frame_token) {
                    return Err(BrowserError::StalePage);
                }
                (
                    json!({"op":"workspace_tab_selected","tab_id":tab_id,"frame_token":frame_token}),
                    MAX_RESPONSE,
                    selected_observation,
                )
            }
            TabCommand::Input {
                tab_id,
                frame_token,
                input,
            } => {
                input.validate()?;
                if !hex64(&frame_token) {
                    return Err(BrowserError::StalePage);
                }
                (
                    json!({"op":"workspace_tab_input","tab_id":tab_id,"frame_token":frame_token,"input":input}),
                    2048,
                    |v| {
                        if v == json!({"accepted":true}) {
                            Ok(v)
                        } else {
                            Err(BrowserError::Protocol)
                        }
                    },
                )
            }
            TabCommand::Stop => return Err(BrowserError::InvalidHandle),
        };
        let value = self.request(body, limit).await?;
        self.checked(check(value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn member_protocol_rejects_generic_commands_and_confused_tab_lists() {
        assert!(
            serde_json::from_value::<TabCommand>(json!({"op":"evaluate","script":"1"})).is_err()
        );
        assert!(
            serde_json::from_value::<TabCommand>(
                json!({"op":"frame","tab_id":Uuid::new_v4(),"url":"file:///tmp"})
            )
            .is_err()
        );
        assert!(validated_tabs(json!({"active_tab":Uuid::new_v4(),"tabs":[]})).is_err());
        let tab = Uuid::new_v4();
        assert!(
            validated_tabs(json!({"active_tab":tab,"tabs":[{"tab_id":tab,"url":"file:///tmp"}]}))
                .is_err()
        );
    }
}
