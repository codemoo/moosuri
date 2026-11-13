//! Private CDP-backed implementation used only by the explicitly selected worker mode.
//! Public human controller types live in `capsule`; no arbitrary CDP/script API is exposed.
use super::*;
use capsule::{
    RenderedLink, RenderedPage, WorkspaceFrame, WorkspaceInput, WorkspaceKey, WorkspaceObservation,
};
use sha2::{Digest, Sha256};

const MAXIMUM: Duration = Duration::from_secs(capsule::workspace::WORKSPACE_MAX_SECONDS);
const FRAME_MAX_AGE: Duration = Duration::from_secs(30);
const INTERACTION_STYLES: [&str; 8] = [
    "display",
    "visibility",
    "pointer-events",
    "opacity",
    "z-index",
    "position",
    "overflow",
    "transform",
];
#[derive(Clone, PartialEq, Eq)]
struct Identity {
    frame: String,
    loader: String,
    url: String,
}
struct FrameProof {
    token: String,
    identity: Identity,
    geometry: String,
    captured_at: Instant,
}
pub struct BrowserWorkspace {
    session: BrowserSession,
    current: Option<PageHandle>,
    proof: Option<FrameProof>,
}
macro_rules! bounded {
    ($this:ident, $operation:expr) => {{
        let duration = $this.session.remaining()?;
        let cancellation = $this.session.cancellation.clone();
        let result = tokio::select! { biased;
            _ = cancellation.cancelled() => Err(BrowserError::Cancelled),
            result = tokio::time::timeout(duration, $operation) => result.unwrap_or(Err(BrowserError::Timeout)),
        };
        $this.session.after(&result);
        result
    }};
}
impl BrowserWorkspace {
    pub async fn launch(
        config: BrowserConfig,
        cancellation: CancellationToken,
    ) -> Result<Self, BrowserError> {
        Ok(Self {
            session: BrowserSession::launch_bounded(config, cancellation, MAXIMUM).await?,
            current: None,
            proof: None,
        })
    }
    pub fn expires_in(&self) -> Duration {
        self.session.expires_in()
    }
    pub async fn shutdown(self) {
        self.session.shutdown().await;
    }
    pub async fn open(&mut self, url: &str) -> Result<WorkspaceObservation, BrowserError> {
        let url = net::public_url(url)?.to_string();
        bounded!(self, self.open_inner(&url))
    }
    async fn open_inner(&mut self, url: &str) -> Result<WorkspaceObservation, BrowserError> {
        self.proof = None;
        if self.current.is_none() {
            self.current = Some(self.session.create_page(url).await?);
        } else {
            let sid = self.sid()?;
            self.session.cdp.before_navigation(&sid);
            let navigation = self
                .session
                .cdp
                .command("Page.navigate", json!({"url":url}), Some(&sid))
                .await?;
            if navigation.get("errorText").is_some() {
                return Err(BrowserError::Network);
            }
            // Same-document navigation has no loaderId and no load event.
            if let Some(loader) = navigation.get("loaderId").and_then(Value::as_str) {
                self.session.cdp.wait_loaded(&sid, loader).await?;
            }
        }
        self.observe_inner().await
    }
    pub async fn observe(&mut self) -> Result<WorkspaceObservation, BrowserError> {
        bounded!(self, self.observe_inner())
    }
    fn sid(&self) -> Result<String, BrowserError> {
        let handle = self.current.as_ref().ok_or(BrowserError::InvalidHandle)?;
        self.session
            .targets
            .get(&handle.page)
            .map(|t| t.session.clone())
            .ok_or(BrowserError::InvalidHandle)
    }
    async fn refresh(&mut self) -> Result<Identity, BrowserError> {
        let sid = self.sid()?;
        let tree = self
            .session
            .cdp
            .command("Page.getFrameTree", json!({}), Some(&sid))
            .await?;
        let frame = tree
            .pointer("/frameTree/frame")
            .ok_or(BrowserError::Protocol)?;
        let identity = Identity {
            frame: string(frame, "id")?,
            loader: string(frame, "loaderId")?,
            url: net::public_url(&string(frame, "url")?)?.to_string(),
        };
        let handle = self.current.as_ref().ok_or(BrowserError::InvalidHandle)?;
        let target = self
            .session
            .targets
            .get_mut(&handle.page)
            .ok_or(BrowserError::InvalidHandle)?;
        target.frame = identity.frame.clone();
        target.loader = identity.loader.clone();
        if self.proof.as_ref().is_some_and(|p| p.identity != identity) {
            self.proof = None;
        }
        Ok(identity)
    }
    async fn observe_inner(&mut self) -> Result<WorkspaceObservation, BrowserError> {
        self.proof = None;
        let identity = self.refresh().await?;
        let sid = self.sid()?;
        let ax = self
            .session
            .cdp
            .command("Accessibility.getFullAXTree", json!({}), Some(&sid))
            .await?;
        let snapshot = self.session.cdp.command("DOMSnapshot.captureSnapshot", json!({"computedStyles":["display","visibility","opacity"],"includePaintOrder":true,"includeDOMRects":true}), Some(&sid)).await?;
        let observation = project_observation(&identity.url, &ax, &snapshot)?;
        if self.refresh().await? != identity {
            return Err(BrowserError::StalePage);
        }
        Ok(observation)
    }
    pub async fn frame(&mut self) -> Result<WorkspaceFrame, BrowserError> {
        bounded!(self, self.frame_inner())
    }
    async fn geometry(&mut self) -> Result<(Identity, String), BrowserError> {
        let identity = self.refresh().await?;
        let sid = self.sid()?;
        let snapshot = self
            .session
            .cdp
            .command(
                "DOMSnapshot.captureSnapshot",
                json!({"computedStyles":INTERACTION_STYLES,"includeDOMRects":true,"includePaintOrder":true}),
                Some(&sid),
            )
            .await?;
        let metrics = self
            .session
            .cdp
            .command("Page.getLayoutMetrics", json!({}), Some(&sid))
            .await?;
        let ax = self
            .session
            .cdp
            .command("Accessibility.getFullAXTree", json!({}), Some(&sid))
            .await?;
        let focus = focused_backends(&ax)?;
        let geometry = geometry_fingerprint(&snapshot, &metrics, &focus)?;
        if self.refresh().await? != identity {
            return Err(BrowserError::StalePage);
        }
        Ok((identity, geometry))
    }
    async fn capture(&mut self) -> Result<(Identity, String, String), BrowserError> {
        let (identity, geometry) = self.geometry().await?;
        let sid = self.sid()?;
        let result = self.session.cdp.workspace_screenshot(&sid).await?;
        let data = string(&result, "data")?;
        if data.len() > 3 * 1024 * 1024 {
            return Err(BrowserError::OutputTooLarge);
        }
        let png = base64::engine::general_purpose::STANDARD
            .decode(&data)
            .map_err(|_| BrowserError::Protocol)?;
        if png.len() > 2 * 1024 * 1024
            || png.len() < 24
            || !png.starts_with(b"\x89PNG\r\n\x1a\n")
            || png[12..16] != *b"IHDR"
            || png[16..20] != 1280u32.to_be_bytes()
            || png[20..24] != 900u32.to_be_bytes()
        {
            return Err(BrowserError::Protocol);
        }
        let (after_identity, after_geometry) = self.geometry().await?;
        if after_identity != identity || after_geometry != geometry {
            self.proof = None;
            return Err(BrowserError::StalePage);
        }
        Ok((identity, data, geometry))
    }
    async fn frame_inner(&mut self) -> Result<WorkspaceFrame, BrowserError> {
        self.proof = None;
        let (identity, png_base64, geometry) = self.capture().await?;
        let frame_token = format!(
            "{:x}",
            Sha256::digest(format!("{}:{geometry}", Uuid::new_v4()))
        );
        self.proof = Some(FrameProof {
            token: frame_token.clone(),
            identity: identity.clone(),
            geometry,
            captured_at: Instant::now(),
        });
        Ok(WorkspaceFrame {
            png_base64,
            width: 1280,
            height: 900,
            url: identity.url,
            frame_token,
        })
    }
    /// No observation is produced here: only a human can view the frame after typing.
    pub async fn input(
        &mut self,
        expected_frame_token: &str,
        input: WorkspaceInput,
    ) -> Result<(), BrowserError> {
        input.validate()?;
        bounded!(self, self.input_inner(expected_frame_token, input))
    }
    async fn input_inner(
        &mut self,
        expected: &str,
        input: WorkspaceInput,
    ) -> Result<(), BrowserError> {
        // Consume before any await. Neither stale input nor uncertain dispatch is replayed.
        let proof = self
            .proof
            .take()
            .filter(|p| p.token == expected)
            .ok_or(BrowserError::StalePage)?;
        if proof.captured_at.elapsed() > FRAME_MAX_AGE {
            return Err(BrowserError::StalePage);
        }
        // Pixel equality would reject a blinking caret or a color animation.
        // Compare element bounds, interaction styles/paint order, focus and viewport/scroll state.
        // Text values and caret pixels never enter this proof or an observation.
        let (identity, geometry) = self.geometry().await?;
        if identity != proof.identity
            || geometry != proof.geometry
            || proof.captured_at.elapsed() > FRAME_MAX_AGE
        {
            return Err(BrowserError::StalePage);
        }
        let sid = self.sid()?;
        match input {
            WorkspaceInput::Click { x, y } => {
                self.session
                    .cdp
                    .command(
                        "Input.dispatchMouseEvent",
                        json!({"type":"mousePressed","x":x,"y":y,"button":"left","clickCount":1}),
                        Some(&sid),
                    )
                    .await?;
                self.session
                    .cdp
                    .command(
                        "Input.dispatchMouseEvent",
                        json!({"type":"mouseReleased","x":x,"y":y,"button":"left","clickCount":1}),
                        Some(&sid),
                    )
                    .await?;
            }
            WorkspaceInput::InsertText { mut text } => {
                use zeroize::Zeroize;
                let result = self
                    .session
                    .cdp
                    .command("Input.insertText", json!({"text":text}), Some(&sid))
                    .await;
                text.zeroize();
                result?;
            }
            WorkspaceInput::Key { key } => {
                let (name, code, virtual_code, text) = key_spec(key);
                self.session.cdp.command("Input.dispatchKeyEvent", json!({"type":"keyDown","key":name,"code":code,"windowsVirtualKeyCode":virtual_code,"text":text}), Some(&sid)).await?;
                self.session.cdp.command("Input.dispatchKeyEvent", json!({"type":"keyUp","key":name,"code":code,"windowsVirtualKeyCode":virtual_code}), Some(&sid)).await?;
            }
            WorkspaceInput::Scroll { delta_x, delta_y } => {
                self.session.cdp.command("Input.dispatchMouseEvent", json!({"type":"mouseWheel","x":640,"y":450,"deltaX":delta_x,"deltaY":delta_y}), Some(&sid)).await?;
            }
        }
        // Refresh only identity, never text. The next human frame is an explicit request.
        if let Err(error) = self.refresh().await {
            self.session.poisoned = true;
            self.session.kill();
            return Err(error);
        }
        Ok(())
    }
}
/// A bounded proof of the rendered document geometry. Resolve only the document
/// URL and fixed interaction styles from the string table. Input/text values,
/// color/caret pixels and AX names are never hashed.
fn geometry_fingerprint(
    snapshot: &Value,
    metrics: &Value,
    focus: &[u64],
) -> Result<String, BrowserError> {
    let documents = snapshot["documents"]
        .as_array()
        .ok_or(BrowserError::Protocol)?;
    let strings = snapshot["strings"]
        .as_array()
        .ok_or(BrowserError::Protocol)?;
    if documents.is_empty() || documents.len() > 64 {
        return Err(BrowserError::OutputTooLarge);
    }
    let mut rows = Vec::new();
    for document in documents {
        let nodes = &document["nodes"];
        let types = nodes["nodeType"].as_array().ok_or(BrowserError::Protocol)?;
        let backends = nodes["backendNodeId"]
            .as_array()
            .ok_or(BrowserError::Protocol)?;
        let indexes = document["layout"]["nodeIndex"]
            .as_array()
            .ok_or(BrowserError::Protocol)?;
        let bounds = document["layout"]["bounds"]
            .as_array()
            .ok_or(BrowserError::Protocol)?;
        let styles = document["layout"]["styles"]
            .as_array()
            .ok_or(BrowserError::Protocol)?;
        let paint_orders = document["layout"]["paintOrders"]
            .as_array()
            .ok_or(BrowserError::Protocol)?;
        if types.is_empty()
            || types.len() > 30000
            || backends.len() != types.len()
            || indexes.len() != bounds.len()
            || indexes.len() > 30000
            || styles.len() != indexes.len()
            || paint_orders.len() != indexes.len()
        {
            return Err(BrowserError::OutputTooLarge);
        }
        let url = required_snapshot_string(strings, &document["documentURL"])?;
        let frame = required_snapshot_string(strings, &document["frameId"])?;
        let scroll = [
            number(&document["scrollOffsetX"])?,
            number(&document["scrollOffsetY"])?,
        ];
        rows.push(json!([frame, backends[0], url, scroll]));
        for (offset, (index, bound)) in indexes.iter().zip(bounds).enumerate() {
            let index = index.as_u64().ok_or(BrowserError::Protocol)? as usize;
            let node_type = types
                .get(index)
                .and_then(Value::as_u64)
                .ok_or(BrowserError::Protocol)?;
            // Element boxes define interaction geometry. Text nodes may change
            // without moving elements (clocks, a typed value or caret animation).
            if node_type != 1 {
                continue;
            }
            let bound = bound
                .as_array()
                .filter(|v| v.len() == 4)
                .ok_or(BrowserError::Protocol)?;
            let rect = bound.iter().map(number).collect::<Result<Vec<_>, _>>()?;
            let backend = backends[index].as_u64().ok_or(BrowserError::Protocol)?;
            let style = styles[offset]
                .as_array()
                .filter(|v| v.len() == INTERACTION_STYLES.len())
                .ok_or(BrowserError::Protocol)?;
            let style = style
                .iter()
                .map(|index| snapshot_string(strings, index))
                .collect::<Result<Vec<_>, _>>()?;
            let paint_order = paint_orders[offset]
                .as_u64()
                .ok_or(BrowserError::Protocol)?;
            rows.push(json!([backend, rect, style, paint_order]));
        }
    }
    for (key, fields) in [
        (
            "cssLayoutViewport",
            &["pageX", "pageY", "clientWidth", "clientHeight"][..],
        ),
        (
            "cssVisualViewport",
            &[
                "offsetX",
                "offsetY",
                "pageX",
                "pageY",
                "clientWidth",
                "clientHeight",
                "scale",
            ][..],
        ),
        ("cssContentSize", &["x", "y", "width", "height"][..]),
    ] {
        let values = fields
            .iter()
            .map(|field| number(&metrics[key][field]))
            .collect::<Result<Vec<_>, _>>()?;
        rows.push(json!([key, values]));
    }
    rows.push(json!(["focused_backend_ids", focus]));
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&rows).map_err(|_| BrowserError::Protocol)?)
    ))
}
/// Only focused backend identities cross into the proof. AX values/names are
/// neither projected nor retained by this path.
fn focused_backends(ax: &Value) -> Result<Vec<u64>, BrowserError> {
    let nodes = ax["nodes"].as_array().ok_or(BrowserError::Protocol)?;
    if nodes.len() > 30000 {
        return Err(BrowserError::OutputTooLarge);
    }
    let mut focused = Vec::new();
    for node in nodes {
        let properties = node.get("properties").and_then(Value::as_array);
        if properties.is_some_and(|properties| {
            properties.iter().any(|p| {
                p["name"] == "focused" && p.pointer("/value/value") == Some(&Value::Bool(true))
            })
        }) {
            focused.push(
                node["backendDOMNodeId"]
                    .as_u64()
                    .ok_or(BrowserError::Protocol)?,
            );
        }
    }
    focused.sort_unstable();
    focused.dedup();
    Ok(focused)
}
/// DOMSnapshot uses -1 for an absent/empty string (notably layout.text,
/// empty attribute values and empty computed-style values). Other negative and
/// out-of-range positive indexes are malformed. Required identities/names use
/// the stricter wrapper rather than accepting an absent value.
fn snapshot_string<'a>(
    strings: &'a [Value],
    index: &Value,
) -> Result<Option<&'a str>, BrowserError> {
    match index.as_i64() {
        Some(-1) => Ok(None),
        Some(index) if index >= 0 => strings
            .get(index as usize)
            .and_then(Value::as_str)
            .map(Some)
            .ok_or(BrowserError::Protocol),
        _ => Err(BrowserError::Protocol),
    }
}
fn required_snapshot_string<'a>(
    strings: &'a [Value],
    index: &Value,
) -> Result<&'a str, BrowserError> {
    snapshot_string(strings, index)?.ok_or(BrowserError::Protocol)
}
fn number(value: &Value) -> Result<f64, BrowserError> {
    value
        .as_f64()
        .filter(|v| v.is_finite() && v.abs() <= 1_000_000_000.0)
        .ok_or(BrowserError::Protocol)
}

fn key_spec(key: WorkspaceKey) -> (&'static str, &'static str, u32, &'static str) {
    match key {
        WorkspaceKey::Enter => ("Enter", "Enter", 13, "\r"),
        WorkspaceKey::Tab => ("Tab", "Tab", 9, ""),
        WorkspaceKey::Backspace => ("Backspace", "Backspace", 8, ""),
        WorkspaceKey::Escape => ("Escape", "Escape", 27, ""),
        WorkspaceKey::ArrowLeft => ("ArrowLeft", "ArrowLeft", 37, ""),
        WorkspaceKey::ArrowRight => ("ArrowRight", "ArrowRight", 39, ""),
        WorkspaceKey::ArrowUp => ("ArrowUp", "ArrowUp", 38, ""),
        WorkspaceKey::ArrowDown => ("ArrowDown", "ArrowDown", 40, ""),
        WorkspaceKey::Delete => ("Delete", "Delete", 46, ""),
        WorkspaceKey::Home => ("Home", "Home", 36, ""),
        WorkspaceKey::End => ("End", "End", 35, ""),
        WorkspaceKey::PageUp => ("PageUp", "PageUp", 33, ""),
        WorkspaceKey::PageDown => ("PageDown", "PageDown", 34, ""),
        WorkspaceKey::Space => (" ", "Space", 32, " "),
    }
}

/// Read only explicit AX fields and visible layout text. Never serialize raw AX,
/// DOM attributes, inputValue, textValue, or editable/control subtree contents.
/// A site may independently echo a secret into ordinary page text; this is not a
/// universal secret-redaction claim. Observations require explicit broker resume.
fn project_observation(
    url: &str,
    ax: &Value,
    snapshot: &Value,
) -> Result<WorkspaceObservation, BrowserError> {
    let strings = snapshot["strings"]
        .as_array()
        .ok_or(BrowserError::Protocol)?;
    let lookup = |index: &Value| required_snapshot_string(strings, index);
    let optional = |index: &Value| snapshot_string(strings, index).map(|value| value.unwrap_or(""));
    let documents = snapshot["documents"]
        .as_array()
        .ok_or(BrowserError::Protocol)?;
    if documents.is_empty() || documents.len() > 64 {
        return Err(BrowserError::OutputTooLarge);
    }
    let mut excluded_backend = std::collections::HashSet::new();
    let mut visible = String::new();
    let mut links = Vec::new();
    let mut truncated = false;
    for (doc_index, document) in documents.iter().enumerate() {
        let nodes = &document["nodes"];
        let names = nodes["nodeName"].as_array().ok_or(BrowserError::Protocol)?;
        let parents = nodes["parentIndex"]
            .as_array()
            .ok_or(BrowserError::Protocol)?;
        let attributes = nodes["attributes"]
            .as_array()
            .ok_or(BrowserError::Protocol)?;
        let backends = nodes["backendNodeId"]
            .as_array()
            .ok_or(BrowserError::Protocol)?;
        if names.len() > 30000
            || parents.len() != names.len()
            || attributes.len() != names.len()
            || backends.len() != names.len()
        {
            return Err(BrowserError::OutputTooLarge);
        }
        let mut excluded = Vec::with_capacity(names.len());
        for (index, name) in names.iter().enumerate() {
            let name = lookup(name)?;
            let parent = parents[index].as_i64().ok_or(BrowserError::Protocol)?;
            if parent >= index as i64 || parent < -1 {
                return Err(BrowserError::Protocol);
            }
            let attrs = attributes[index].as_array().ok_or(BrowserError::Protocol)?;
            if attrs.len() % 2 != 0 {
                return Err(BrowserError::Protocol);
            }
            let attrs = attrs
                .chunks_exact(2)
                .map(|pair| Ok((lookup(&pair[0])?, optional(&pair[1])?)))
                .collect::<Result<Vec<_>, BrowserError>>()?;
            let control = matches!(
                name,
                "INPUT"
                    | "TEXTAREA"
                    | "SELECT"
                    | "OPTION"
                    | "SCRIPT"
                    | "STYLE"
                    | "NOSCRIPT"
                    | "TEMPLATE"
            ) || attrs.iter().any(|(key, value)| {
                key.eq_ignore_ascii_case("contenteditable")
                    || (key.eq_ignore_ascii_case("role")
                        && matches!(*value, "textbox" | "searchbox" | "combobox" | "spinbutton"))
            });
            let hidden = control || (parent >= 0 && excluded[parent as usize]);
            excluded.push(hidden);
            if hidden {
                excluded_backend.insert(backends[index].as_u64().ok_or(BrowserError::Protocol)?);
            }
            if doc_index == 0 && !hidden && name == "A" && links.len() < 4 {
                for (key, value) in &attrs {
                    if *key == "href" {
                        let base = net::public_url(url)?;
                        if let Ok(link_url) = base.join(value)
                            && let Ok(link_url) = net::public_url(link_url.as_str())
                        {
                            links.push(RenderedLink {
                                text: String::new(),
                                url: link_url.to_string(),
                            });
                        }
                    }
                }
            }
        }
        // Child documents are deliberately omitted: cross-origin frame text is not
        // needed for the narrow main-page observation and may contain credentials.
        if doc_index != 0 {
            continue;
        }
        let layout = &document["layout"];
        let indexes = layout["nodeIndex"]
            .as_array()
            .ok_or(BrowserError::Protocol)?;
        let texts = layout["text"].as_array().ok_or(BrowserError::Protocol)?;
        let styles = layout["styles"].as_array().ok_or(BrowserError::Protocol)?;
        if texts.len() != indexes.len() || styles.len() != indexes.len() {
            return Err(BrowserError::Protocol);
        }
        for (offset, node_index) in indexes.iter().enumerate() {
            let index = node_index.as_u64().ok_or(BrowserError::Protocol)? as usize;
            if *excluded.get(index).ok_or(BrowserError::Protocol)? {
                continue;
            }
            let text = optional(&texts[offset])?;
            // Chromium emits [] styles for nontext document/root layout boxes.
            // Such rows carry no visible text to project. Nonempty text still
            // requires the complete requested visibility style tuple below.
            if text.trim().is_empty() {
                continue;
            }
            let style = styles[offset].as_array().ok_or(BrowserError::Protocol)?;
            if style.len() != 3 {
                return Err(BrowserError::Protocol);
            }
            if optional(&style[0])? == "none"
                || optional(&style[1])? != "visible"
                || optional(&style[2])?.parse::<f64>().is_ok_and(|v| v <= 0.0)
            {
                continue;
            }
            if visible.len() + text.len() > 4800 {
                truncated = true;
                break;
            }
            visible.push_str(text);
            visible.push(' ');
        }
    }
    let nodes = ax["nodes"].as_array().ok_or(BrowserError::Protocol)?;
    if nodes.len() > 30000 {
        return Err(BrowserError::OutputTooLarge);
    }
    // AX virtual descendants can omit backendDOMNodeId. Exclude the entire AX
    // subtree of editable/form nodes as well as matching DOM nodes.
    let mut children: HashMap<&str, Vec<&str>> = HashMap::new();
    let mut parents: HashMap<&str, &str> = HashMap::new();
    let mut excluded_ax = std::collections::HashSet::new();
    let mut pending = Vec::new();
    for node in nodes {
        let id = node["nodeId"].as_str().ok_or(BrowserError::Protocol)?;
        if let Some(parent) = node["parentId"].as_str() {
            children.entry(parent).or_default().push(id);
            parents.insert(id, parent);
        }
        if let Some(ids) = node["childIds"].as_array() {
            for child in ids {
                let child = child.as_str().ok_or(BrowserError::Protocol)?;
                children.entry(id).or_default().push(child);
                parents.insert(child, id);
            }
        }
        let role = node
            .pointer("/role/value")
            .and_then(Value::as_str)
            .unwrap_or("");
        if matches!(role, "textbox" | "searchbox" | "combobox" | "spinbutton")
            || node["backendDOMNodeId"]
                .as_u64()
                .is_some_and(|id| excluded_backend.contains(&id))
        {
            pending.push(id);
        }
    }
    while let Some(id) = pending.pop() {
        if excluded_ax.insert(id)
            && let Some(ids) = children.get(id)
        {
            pending.extend(ids);
        }
    }
    let mut title = String::new();
    let mut tree = String::new();
    for node in nodes {
        if node["ignored"] == true
            || node["nodeId"]
                .as_str()
                .is_some_and(|id| excluded_ax.contains(id))
        {
            continue;
        }
        let role = node
            .pointer("/role/value")
            .and_then(Value::as_str)
            .unwrap_or("");
        if role == "RootWebArea" && title.is_empty() {
            title = node
                .pointer("/name/value")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned();
            trim_bytes(&mut title, 512);
        }
        if !matches!(
            role,
            "RootWebArea"
                | "heading"
                | "StaticText"
                | "link"
                | "button"
                | "list"
                | "listitem"
                | "table"
                | "row"
                | "cell"
                | "columnheader"
                | "rowheader"
                | "alert"
                | "status"
        ) {
            continue;
        }
        if node
            .get("backendDOMNodeId")
            .and_then(Value::as_u64)
            .is_some_and(|id| excluded_backend.contains(&id))
        {
            continue;
        }
        let mut name = node
            .pointer("/name/value")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        trim_bytes(&mut name, 256);
        if name.is_empty() {
            continue;
        }
        let id = node["nodeId"].as_str().ok_or(BrowserError::Protocol)?;
        let row = json!({"node":id,"parent":parents.get(id),"role":role,"name":name}).to_string();
        if tree.len() + row.len() > 2800 {
            truncated = true;
            break;
        }
        tree.push_str(&row);
        tree.push('\n');
    }
    let mut text = format!("Accessibility:\n{tree}\nVisible layout text:\n{visible}");
    if text.len() > 8192 {
        truncated = true;
        trim_bytes(&mut text, 8192);
    }
    let retrieved_at_unix_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| BrowserError::Setup)?
        .as_millis()
        .try_into()
        .map_err(|_| BrowserError::Setup)?;
    Ok(WorkspaceObservation { page:RenderedPage { url:url.to_owned(), title, text, links, truncated,
        warning:"isolated_task_workspace; untrusted_page_content; form_values_omitted; page_echoes_may_contain_secrets".into(), retrieved_at_unix_ms } })
}

#[cfg(test)]
mod tests;
