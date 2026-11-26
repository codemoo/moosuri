//! Reserves a guest-only recipient. No Chrome or plaintext exists at this stage.
use super::*;
use moosuri_connectors::browser::google_auth::{Binding, PROTOCOL as RUNTIME_PROTOCOL, Recipient};

#[derive(Clone)]
pub(in super::super) struct Runtime {
    socket: PathBuf,
    installation: Uuid,
    starts: Arc<Mutex<std::collections::HashMap<String, Vec<Instant>>>>,
}
impl Runtime {
    pub(in super::super) fn new(options: &Options) -> Result<Option<Self>, String> {
        match (
            &options.google_runtime_socket,
            options.google_runtime_installation,
        ) {
            (None, None) => Ok(None),
            (Some(socket), Some(installation))
                if socket.is_absolute()
                    && socket.as_os_str().len() < 100
                    && !installation.is_nil() =>
            {
                Ok(Some(Self {
                    socket: socket.clone(),
                    installation,
                    starts: Default::default(),
                }))
            }
            _ => Err("google_runtime_configuration_invalid".into()),
        }
    }

    pub(super) fn admit(&self, account: &str) -> Result<(), StatusCode> {
        let now = Instant::now();
        let mut starts = self.starts.lock().map_err(|_| UNAVAILABLE)?;
        starts.retain(|_, times| {
            times.retain(|at| now.duration_since(*at) < Duration::from_secs(60));
            !times.is_empty()
        });
        if !starts.contains_key(account) && starts.len() >= 1024 {
            return Err(StatusCode::TOO_MANY_REQUESTS);
        }
        let times = starts.entry(account.to_owned()).or_default();
        if times.len() >= 4 {
            return Err(StatusCode::TOO_MANY_REQUESTS);
        }
        times.push(now);
        Ok(())
    }
}
pub(super) struct Prepared {
    pub(super) peer: BufReader<UnixStream>,
    recipient: Recipient,
}

impl Prepared {
    pub(super) fn binding(&self) -> &Binding {
        &self.recipient.binding
    }
    fn alive(&mut self) -> bool {
        // No bytes are expected between the recipient and Deliver. Readiness
        // here means EOF or an unsolicited terminal broker receipt.
        self.peer.buffer().is_empty()
            && matches!(self.peer.get_mut().try_read(&mut [0u8; 1]),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock)
    }
}
fn release_idle(g: &Gateway, id: Uuid, challenge: Uuid) {
    // Expiration is itself a reason to release: change() cannot edit expired jobs.
    if let Ok(mut jobs) = g.google_jobs.lock() {
        if let Some(j) = jobs
            .iter_mut()
            .find(|j| j.id == id && j.challenge == challenge && j.state == "vault")
        {
            j.prepared = None;
            finish_run(j, "unavailable", true, Instant::now());
        }
    }
}

async fn open(
    runtime: &Runtime,
    binding: Binding,
    service: Option<&str>,
) -> Result<Prepared, StatusCode> {
    let stream = UnixStream::connect(&runtime.socket)
        .await
        .map_err(|_| UNAVAILABLE)?;
    if stream.peer_cred().map_err(|_| UNAVAILABLE)?.uid() != 0 {
        return Err(UNAVAILABLE);
    }
    let mut peer = BufReader::new(stream);
    send(
        &mut peer,
        json!({"protocol":RUNTIME_PROTOCOL,"binding":binding,"service":service.unwrap_or("gmail")}),
    )
    .await?;
    let mut bytes = Vec::new();
    (&mut peer)
        .take(4097)
        .read_until(b'\n', &mut bytes)
        .await
        .map_err(|_| UNAVAILABLE)?;
    if bytes.len() > 4096 || bytes.pop() != Some(b'\n') {
        return Err(UNAVAILABLE);
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Ready {
        status: String,
        recipient: Recipient,
    }
    let ready: Ready = serde_json::from_slice(&bytes).map_err(|_| UNAVAILABLE)?;
    if ready.status != "vault"
        || ready.recipient.protocol != RUNTIME_PROTOCOL
        || ready.recipient.binding != binding
    {
        return Err(UNAVAILABLE);
    }
    Ok(Prepared {
        peer,
        recipient: ready.recipient,
    })
}
pub(super) fn prepare(
    g: &Gateway,
    a: &SessionAllocation,
    session: &str,
    id: Uuid,
    challenge: Uuid,
    email: &str,
    service: Option<&'static str>,
    jobs: &mut [Job],
) -> Result<(), StatusCode> {
    let runtime = g.google_runtime.clone().ok_or(UNAVAILABLE)?;
    let binding = Binding {
        installation: runtime.installation,
        account: a.account_id.parse().map_err(|_| UNAVAILABLE)?,
        tenant: a.tenant_id.parse().map_err(|_| UNAVAILABLE)?,
        volume: a.volume_id.parse().map_err(|_| UNAVAILABLE)?,
        generation: a.generation,
        epoch: a.epoch,
        challenge,
        email: email.into(),
        origin: g.origin.clone(),
    };
    let job = jobs
        .iter_mut()
        .find(|j| j.id == id && j.challenge == challenge)
        .ok_or(GONE)?;
    let (stop, cancelled) = oneshot::channel();
    job.stop = Some(stop);
    let end = job.expires;
    // A prepared key has no plaintext or Chrome. Only Deliver owns the live
    // login fence. Dropping this peer cancels its key-only capsule; the engine
    // inventory still prevents oversubscription while cleanup finishes.
    let gateway = g.clone();
    let a = SessionAllocation {
        allocation: a.allocation.clone(),
        epoch: a.epoch,
    };
    let session = session.to_owned();
    let email = email.to_owned();
    tokio::spawn(async move {
        let result = tokio::select! {biased;
            _=cancelled=>Err(GONE),
            result=timeout(Duration::from_secs(50),async {
                let prepared=open(&runtime,binding,service).await?;
                reauthorize(&gateway,&a,&session,&email).await?;
                Ok(prepared)
            })=>result.unwrap_or(Err(UNAVAILABLE)),
        };
        let installed = change(&gateway, id, challenge, |j| {
            if j.state != "preparing" {
                return false;
            }
            j.stop = None;
            match result {
                Ok(prepared) => {
                    j.prepared = Some(prepared);
                    j.state = "vault";
                    true
                }
                Err(_) => {
                    finish_run(j, "unavailable", true, Instant::now());
                    false
                }
            }
        });
        if installed != Ok(true) {
            return;
        }
        // A logout without a final browser Cancel must not occupy a guest key
        // slot until the vault timeout. Deliver hands authorization monitoring
        // to the existing run task; this task never owns or reads ciphertext.
        loop {
            tokio::time::sleep(AUTH_RECHECK_INTERVAL).await;
            if Instant::now() >= end {
                release_idle(&gateway, id, challenge);
                break;
            }
            if change(&gateway, id, challenge, |j| {
                j.state == "vault" && j.prepared.as_mut().is_some_and(Prepared::alive)
            }) != Ok(true)
            {
                release_idle(&gateway, id, challenge);
                break;
            }
            let remaining = end.saturating_duration_since(Instant::now());
            if !matches!(
                timeout(
                    remaining.min(Duration::from_secs(5)),
                    reauthorize(&gateway, &a, &session, &email)
                )
                .await,
                Ok(Ok(()))
            ) {
                release_idle(&gateway, id, challenge);
                break;
            }
        }
    });
    Ok(())
}

pub(in super::super) fn recipient(
    g: &Gateway,
    a: &SessionAllocation,
    challenge: Uuid,
) -> Result<Value, StatusCode> {
    let jobs = g.google_jobs.lock().map_err(|_| UNAVAILABLE)?;
    let job = jobs
        .iter()
        .find(|j| {
            j.challenge == challenge
                && j.allocation == a.allocation
                && j.epoch == a.epoch
                && j.state == "vault"
                && j.expires > Instant::now()
        })
        .ok_or(GONE)?;
    serde_json::to_value(&job.prepared.as_ref().ok_or(GONE)?.recipient).map_err(|_| UNAVAILABLE)
}
