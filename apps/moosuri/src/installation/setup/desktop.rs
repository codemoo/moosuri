//! A local status/control surface; account data and all settings remain in the web app.
mod integration;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(test)]
mod tests;
use super::*;
pub(super) use integration::{launch_if_installed, offer, preflight_remove, remove};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::time::Instant;

#[derive(Clone, Copy, clap::ValueEnum)]
pub enum Switch {
    On,
    Off,
}
#[derive(Args)]
pub struct Autostart {
    #[arg(long)]
    pub prefix: Option<PathBuf>,
    #[arg(value_enum)]
    pub mode: Switch,
}
pub fn autostart(options: &Autostart) -> Result<()> {
    let p = prefix(&options.prefix)?;
    change_autostart(&p, matches!(options.mode, Switch::On))
}
fn change_autostart(p: &Path, enabled: bool) -> Result<()> {
    let _control = control_lock(p)?;
    check_start_allowed(p)?;
    let mut state = load(p)?;
    service::set_autostart(p, &state, enabled)?;
    state.autostart = enabled;
    save(p, &state)
}
fn named_lock(prefix: &Path, name: &str) -> Result<File> {
    private_dir(prefix)?;
    let p = prefix.join(name);
    if !p.exists() {
        match create(&p, b"", false) {
            Ok(()) => (),
            Err(_) if p.exists() => (),
            Err(e) => return Err(e),
        }
    }
    read(&p, 0, true)?;
    let f = OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(p)
        .map_err(error)?;
    acl_supported(&f, false)?;
    f.try_lock_exclusive()
        .map_err(|_| "setup_control_busy".to_string())?;
    Ok(f)
}
pub(in crate::installation) fn control_lock(prefix: &Path) -> Result<File> {
    named_lock(prefix, "control.lock")
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Phase {
    Checking,
    Ready,
    Starting,
    Stopping,
    Restarting,
    Stopped,
    Error,
}
impl Phase {
    fn title(self, l: Language) -> &'static str {
        let (en, ko) = match self {
            Self::Checking => ("Checking server…", "서버 확인 중…"),
            Self::Ready => ("Server running", "서버 실행 중"),
            Self::Starting => ("Starting server…", "서버 시작 중…"),
            Self::Restarting => ("Restarting server…", "서버 재시작 중…"),
            Self::Stopping => ("Stopping server…", "서버 중지 중…"),
            Self::Stopped => ("Server stopped", "서버 중지됨"),
            Self::Error => ("Server needs attention", "서버 확인 필요"),
        };
        l.text(en, ko)
    }
    fn busy(self) -> bool {
        matches!(
            self,
            Self::Checking | Self::Starting | Self::Stopping | Self::Restarting
        )
    }
}
#[derive(Clone, PartialEq, Eq)]
struct Snapshot {
    phase: Phase,
    controllable: bool,
    autostart: bool,
    detail: &'static str,
    exit: bool,
}
impl Snapshot {
    fn can_open(&self) -> bool {
        self.phase == Phase::Ready
            || (self.controllable && !self.phase.busy() && self.detail != "port_conflict")
    }
    fn chat_title(&self, language: Language) -> &'static str {
        if self.phase == Phase::Ready {
            language.text("Open Moosuri", "무수리 열기")
        } else {
            language.text("Start and open Moosuri", "시작하고 무수리 열기")
        }
    }
}
impl Default for Snapshot {
    fn default() -> Self {
        Self {
            phase: Phase::Checking,
            controllable: false,
            autostart: false,
            detail: "",
            exit: false,
        }
    }
}
fn classify(service_running: bool, observed: Observation, controllable: bool) -> Phase {
    if observed == Observation::OtherServer {
        Phase::Error
    } else if observed == Observation::Ready {
        Phase::Ready
    } else if service_running {
        Phase::Error
    } else if controllable {
        Phase::Stopped
    } else {
        Phase::Error
    }
}
async fn inspect(p: &Path) -> Result<Snapshot> {
    let s = load(p)?;
    if s.removing || current(p)?.version != own_probe().version {
        return Ok(Snapshot {
            exit: true,
            ..Snapshot::default()
        });
    }
    no_pending(p)?;
    let i = installation(p)?;
    let controllable = !matches!(s.service, service::Kind::None);
    if controllable {
        service::verify(p, &s)?;
    }
    let active = service::running(p, &s);
    let observed = Readiness::new(&i, &s)?.observe().await;
    Ok(Snapshot {
        phase: classify(active, observed, controllable),
        controllable,
        autostart: s.autostart,
        detail: if observed == Observation::OtherServer {
            "port_conflict"
        } else if !controllable {
            "foreground"
        } else if active && observed != Observation::Ready {
            "not_ready"
        } else {
            ""
        },
        exit: false,
    })
}
#[derive(Clone, Copy)]
enum Action {
    Start,
    Restart,
    Stop,
    Autostart(bool),
    Chat,
    Admin,
    Help,
    Quit,
}
#[derive(Clone)]
struct Controller {
    sender: mpsc::SyncSender<Action>,
    state: Arc<Mutex<Snapshot>>,
    quitting: Arc<AtomicBool>,
    language: Language,
}
impl Controller {
    fn send(&self, action: Action) {
        if matches!(action, Action::Quit) {
            self.quitting.store(true, Ordering::Release);
            set_state(&self.state, |s| s.exit = true);
        } else {
            let s = self.snapshot();
            if (matches!(action, Action::Admin) && s.phase != Phase::Ready)
                || (matches!(action, Action::Chat) && !s.can_open())
            {
                return;
            }
        }
        let _ = self.sender.try_send(action);
    }
    fn snapshot(&self) -> Snapshot {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
    fn label<'a>(&self, en: &'a str, ko: &'a str) -> &'a str {
        self.language.text(en, ko)
    }
    fn detail(&self, s: &Snapshot) -> &'static str {
        if s.detail.is_empty() {
            self.label(
                "Web settings require your normal sign-in",
                "웹 설정은 평소처럼 로그인하여 사용합니다",
            )
        } else {
            diagnostics::hint(s.detail, self.language)
        }
    }
}
fn set_state(state: &Arc<Mutex<Snapshot>>, f: impl FnOnce(&mut Snapshot)) {
    f(&mut state.lock().unwrap_or_else(|e| e.into_inner()));
}
fn controller(p: PathBuf, language: Language) -> (Controller, std::thread::JoinHandle<()>) {
    let (sender, receiver) = mpsc::sync_channel(1);
    let state = Arc::new(Mutex::new(Snapshot::default()));
    let quitting = Arc::new(AtomicBool::new(false));
    let c = Controller {
        sender,
        quitting: quitting.clone(),
        state: state.clone(),
        language,
    };
    let worker = std::thread::spawn(move || {
        let Ok(rt) = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        else {
            set_state(&state, |s| {
                s.phase = Phase::Error;
                s.detail = "unavailable";
                s.exit = true;
            });
            return;
        };
        let mut last_error = None;
        while !quitting.load(Ordering::Acquire) {
            match rt.block_on(inspect(&p)) {
                Ok(mut s) => {
                    if let Some(problem) = last_error {
                        s.detail = problem;
                    }
                    let exit = s.exit;
                    set_state(&state, |v| *v = s);
                    if exit {
                        break;
                    }
                }
                Err(e) => {
                    if !p.join("setup.json").exists() {
                        set_state(&state, |s| s.exit = true);
                        break;
                    }
                    set_state(&state, |s| {
                        s.phase = Phase::Error;
                        s.controllable = false;
                        s.detail = diagnostics::classify(&e);
                    });
                }
            }
            if quitting.load(Ordering::Acquire) {
                break;
            }
            let action = match receiver.recv_timeout(Duration::from_secs(3)) {
                Ok(Action::Quit) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Ok(a) => a,
            };
            if quitting.load(Ordering::Acquire) {
                break;
            }
            set_state(&state, |s| {
                s.phase = match action {
                    Action::Start | Action::Chat if s.phase != Phase::Ready => Phase::Starting,
                    Action::Restart => Phase::Restarting,
                    Action::Stop => Phase::Stopping,
                    _ => s.phase,
                }
            });
            let result = match action {
                Action::Start => rt.block_on(start(&Start {
                    prefix: Some(p.clone()),
                    foreground: false,
                })),
                Action::Restart => rt.block_on(restart(&Location {
                    prefix: Some(p.clone()),
                })),
                Action::Stop => stop(&Location {
                    prefix: Some(p.clone()),
                }),
                Action::Autostart(enabled) => change_autostart(&p, enabled),
                Action::Chat => rt.block_on(open(&Open {
                    prefix: Some(p.clone()),
                    start: true,
                    wait_ready: false,
                    page: "/app".into(),
                })),
                Action::Admin => rt.block_on(async {
                    let i = installation(&p)?;
                    let s = load(&p)?;
                    Readiness::new(&i, &s)?.observe().await.require_ready()?;
                    browser(&format!("{}/admin", i.origin))
                }),
                Action::Help => browser(
                    "https://github.com/codemoo/moosuri/blob/main/docs/install.md#desktop-menu--데스크톱-메뉴",
                ),
                Action::Quit => break,
            };
            match result {
                Err(e) => last_error = Some(diagnostics::classify(&e)),
                Ok(()) if !matches!(action, Action::Help) => last_error = None,
                _ => (),
            }
        }
        set_state(&state, |s| s.exit = true);
    });
    (c, worker)
}
pub fn run(location: &Location) -> Result<()> {
    let p = prefix(&location.prefix)?;
    installation(&p)?;
    check_start_allowed(&p)?;
    let s = load(&p)?;
    let _single_instance = match named_lock(&p, "desktop.lock") {
        Ok(f) => f,
        Err(e) if e == "setup_control_busy" => return Ok(()),
        Err(e) => return Err(e),
    };
    let (c, worker) = controller(p, s.language);
    #[cfg(target_os = "macos")]
    let result = macos::run(c.clone());
    #[cfg(target_os = "linux")]
    let result = linux::run(c.clone());
    c.send(Action::Quit);
    drop(c);
    let _ = worker.join();
    result
}
// Reuse the published product artwork; decode once when the menu is created.
fn icon_rgba() -> Vec<u8> {
    use std::io::Cursor;
    let bytes = include_bytes!("../../../../../web/public/moosuri-attendant.png");
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().expect("embedded icon metadata");
    let mut pixels = vec![0; reader.output_buffer_size().expect("embedded icon size")];
    let info = reader
        .next_frame(&mut pixels)
        .expect("embedded icon pixels");
    let channels = info.color_type.samples();
    assert!(channels == 3 || channels == 4);
    let mut icon = Vec::with_capacity(24 * 24 * 4);
    // A small pixel sample keeps the source illustration's deliberately pixelated edges.
    for y in 0..24 {
        for x in 0..24 {
            let sx = ((x * 2 + 1) * info.width as usize / 48).min(info.width as usize - 1);
            let sy = ((y * 2 + 1) * info.height as usize / 48).min(info.height as usize - 1);
            let at = (sy * info.width as usize + sx) * channels;
            icon.extend_from_slice(&pixels[at..at + 3]);
            icon.push(if channels == 4 { pixels[at + 3] } else { 255 });
        }
    }
    icon
}
