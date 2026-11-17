use super::*;
#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum Phase {
    Connecting,
    Connected,
    Disconnecting,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Record {
    format: u32,
    installation_id: String,
    backend_port: u16,
    pub request: Request,
    previous_origin: String,
    previous_network: Network,
    files: BTreeMap<String, [String; 2]>,
    pub phase: Phase,
    pub resume_service: bool,
}
const FILES: [&str; 3] = [
    "deployment/moosuri.service",
    "deployment/proxy.nginx.conf",
    "deployment/tailscale-serve.txt",
];
impl Record {
    pub fn prepare(
        p: &Path,
        i: &Installation,
        s: &State,
        request: Request,
        resume_service: bool,
    ) -> Result<Self> {
        let mut target: Installation = parse(&json(i)?)?;
        target.origin = request.origin();
        target.network = request.network();
        let new = templates(p, &target)?;
        let mut files = BTreeMap::new();
        for name in FILES {
            let old = read(&p.join(name), 65536, true)?;
            if i.managed.get(name) != Some(&hash(&old)) {
                return Err("public_deployment_file_changed_preserved".into());
            }
            files.insert(
                name.into(),
                [
                    String::from_utf8(old).map_err(error)?,
                    new[name].replace("127.0.0.1:3210", &format!("127.0.0.1:{}", s.port)),
                ],
            );
        }
        Ok(Self {
            format: 1,
            installation_id: i.id.clone(),
            backend_port: s.port,
            request,
            previous_origin: i.origin.clone(),
            previous_network: i.network.clone(),
            files,
            phase: Phase::Connecting,
            resume_service,
        })
    }
    pub fn load(p: &Path) -> Result<Option<Self>> {
        match fs::symlink_metadata(p.join(NAME)) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(error(e)),
            Ok(_) => parse(&read(&p.join(NAME), 256 * 1024, true)?).map(Some),
        }
    }
    pub fn save(&self, p: &Path) -> Result<()> {
        atomic(&p.join(NAME), &json(self)?)
    }
    pub fn validate(&self, p: &Path, i: &Installation, s: &State) -> Result<()> {
        self.request.validate()?;
        if self.format != 1
            || self.installation_id != i.id
            || self.backend_port != s.port
            || self.files.len() != 3
            || ![
                self.previous_origin.as_str(),
                self.request.origin().as_str(),
            ]
            .contains(&i.origin.as_str())
            || moosuri_server::ServerConfig::new(
                &self.previous_origin,
                matches!(self.previous_network, Network::Local),
            )
            .is_err()
        {
            return Err("public_connection_receipt_invalid".into());
        }
        for name in FILES {
            let pair = self
                .files
                .get(name)
                .ok_or("public_connection_receipt_invalid")?;
            let actual = read(&p.join(name), 65536, true)?;
            if !pair.iter().any(|v| v.as_bytes() == actual)
                || !pair
                    .iter()
                    .any(|v| i.managed.get(name) == Some(&hash(v.as_bytes())))
            {
                return Err("public_deployment_file_changed_preserved".into());
            }
        }
        Ok(())
    }
    pub fn apply(&self, p: &Path, i: &mut Installation, s: &State, connect: bool) -> Result<()> {
        self.validate(p, i, s)?;
        for (name, pair) in &self.files {
            let value = &pair[usize::from(connect)];
            atomic(&p.join(name), value.as_bytes())?;
            super::super::super::upgrade_fault("public_during_templates")?;
            i.managed.insert(name.clone(), hash(value.as_bytes()));
        }
        i.origin = if connect {
            self.request.origin()
        } else {
            self.previous_origin.clone()
        };
        i.network = if connect {
            self.request.network()
        } else {
            self.previous_network.clone()
        };
        atomic(&p.join("installation.json"), &json(i)?)
    }
    pub fn remove(&self, p: &Path) -> Result<()> {
        let raw = read(&p.join(NAME), 256 * 1024, true)?;
        if !remove_exact(&p.join(NAME), &hash(&raw))? {
            return Err("public_receipt_changed".into());
        }
        sync(p)
    }
}
