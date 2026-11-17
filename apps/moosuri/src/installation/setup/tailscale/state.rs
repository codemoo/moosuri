//! A small durable intent joins external Serve CAS and local origin changes.
use super::*;
const NAME: &str = "tailscale.json";
const EXAMPLES: [&str; 3] = [
    "deployment/moosuri.service",
    "deployment/proxy.nginx.conf",
    "deployment/tailscale-serve.txt",
];
#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum Phase {
    Connecting,
    Connected,
    Disconnecting,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Binding {
    format: u32,
    installation_id: String,
    pub node_id: String,
    pub socket: Option<PathBuf>,
    pub origin: String,
    pub port: u16,
    previous_origin: String,
    previous_network: Network,
    previous_templates: BTreeMap<String, String>,
    target_templates: BTreeMap<String, String>,
    pub phase: Phase,
    pub resume_service: bool,
}
impl Binding {
    pub(super) fn prepare(
        p: &Path,
        i: &Installation,
        s: &State,
        origin: String,
        node_id: String,
        socket: Option<PathBuf>,
        resume_service: bool,
    ) -> Result<Self> {
        let mut old = BTreeMap::new();
        for path in EXAMPLES {
            let bytes = read(&p.join(path), 65536, true)?;
            if i.managed.get(path) != Some(&hash(&bytes)) {
                return Err("tailscale_deployment_file_changed_preserved".into());
            }
            old.insert(path.into(), String::from_utf8(bytes).map_err(error)?);
        }
        let mut target: Installation = parse(&json(i)?)?;
        target.origin = origin.clone();
        target.network = Network::Tailscale;
        let mut target_templates = templates(p, &target)?;
        for text in target_templates.values_mut() {
            *text = text.replace("127.0.0.1:3210", &format!("127.0.0.1:{}", s.port));
        }
        Ok(Self {
            format: 1,
            installation_id: i.id.clone(),
            node_id,
            socket,
            origin,
            port: s.port,
            previous_origin: i.origin.clone(),
            previous_network: i.network.clone(),
            previous_templates: old,
            target_templates,
            phase: Phase::Connecting,
            resume_service,
        })
    }
    pub(super) fn save(&self, p: &Path) -> Result<()> {
        atomic(&p.join(NAME), &json(self)?)
    }
    pub(super) fn validate(&self, p: &Path, i: &Installation, s: &State) -> Result<()> {
        config::host(&self.origin)?;
        if let Some(socket) = &self.socket {
            path_checked(socket)?;
        }
        if self.format != 1
            || self.installation_id != i.id
            || self.node_id.is_empty()
            || self.node_id.len() > 128
            || !self.node_id.is_ascii()
            || self.port != s.port
            || ![self.previous_origin.as_str(), self.origin.as_str()].contains(&i.origin.as_str())
            || moosuri_server::ServerConfig::new(
                &self.previous_origin,
                matches!(self.previous_network, Network::Local),
            )
            .is_err()
            || self.previous_templates.len() != 3
            || self.target_templates.len() != 3
        {
            return Err("tailscale_binding_invalid".into());
        }
        for name in EXAMPLES {
            let old = self
                .previous_templates
                .get(name)
                .ok_or("tailscale_binding_invalid")?;
            let new = self
                .target_templates
                .get(name)
                .ok_or("tailscale_binding_invalid")?;
            let actual = read(&p.join(name), 65536, true)?;
            let saved = i.managed.get(name).ok_or("tailscale_binding_invalid")?;
            if ![old.as_bytes(), new.as_bytes()].contains(&actual.as_slice())
                || ![hash(old.as_bytes()), hash(new.as_bytes())].contains(saved)
            {
                return Err("tailscale_deployment_file_changed_preserved".into());
            }
        }
        Ok(())
    }
    pub(super) fn apply_origin(
        &self,
        p: &Path,
        i: &mut Installation,
        s: &State,
        connect: bool,
    ) -> Result<()> {
        self.validate(p, i, s)?;
        let texts = if connect {
            &self.target_templates
        } else {
            &self.previous_templates
        };
        // Intent stays present throughout. Start admission is closed until all files
        // and the atomic installation record agree; retry accepts either exact version.
        for (name, text) in texts {
            atomic(&p.join(name), text.as_bytes())?;
            super::super::super::upgrade_fault("tailscale_during_templates")?;
            i.managed.insert(name.clone(), hash(text.as_bytes()));
        }
        i.origin = if connect {
            self.origin.clone()
        } else {
            self.previous_origin.clone()
        };
        i.network = if connect {
            Network::Tailscale
        } else {
            self.previous_network.clone()
        };
        atomic(&p.join("installation.json"), &json(i)?)
    }
}
pub(super) fn load_binding(p: &Path) -> Result<Option<Binding>> {
    match fs::symlink_metadata(p.join(NAME)) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(error(e)),
        Ok(_) => parse(&read(&p.join(NAME), 256 * 1024, true)?).map(Some),
    }
}
pub(super) fn remove_binding(p: &Path) -> Result<()> {
    let bytes = read(&p.join(NAME), 256 * 1024, true)?;
    if !remove_exact(&p.join(NAME), &hash(&bytes))? {
        return Err("tailscale_binding_changed".into());
    }
    sync(p)
}
pub(in crate::installation) fn check_start(p: &Path) -> Result<()> {
    if load_binding(p)?.is_some_and(|b| b.phase != Phase::Connected) {
        return Err("tailscale_change_pending_retry_connect_or_disconnect".into());
    }
    Ok(())
}
