use super::*;
use ksni::{
    MenuItem, TrayMethods,
    menu::{CheckmarkItem, StandardItem, SubMenu},
};
struct Tray {
    controller: Controller,
    snapshot: Snapshot,
    icon: Vec<u8>,
}
impl Tray {
    fn item(&self, en: &str, ko: &str, action: Action, enabled: bool) -> MenuItem<Self> {
        StandardItem {
            label: self.controller.label(en, ko).into(),
            enabled,
            activate: Box::new(move |this: &mut Self| this.controller.send(action)),
            ..Default::default()
        }
        .into()
    }
}
impl ksni::Tray for Tray {
    fn id(&self) -> String {
        "io.moosuri.desktop".into()
    }
    fn title(&self) -> String {
        format!(
            "Moosuri · {}",
            self.snapshot.phase.title(self.controller.language)
        )
    }
    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        let mut argb = self.icon.clone();
        for p in argb.chunks_exact_mut(4) {
            p.rotate_right(1);
        }
        vec![ksni::Icon {
            width: 24,
            height: 24,
            data: argb,
        }]
    }
    fn activate(&mut self, _x: i32, _y: i32) {
        self.controller.send(Action::Chat);
    }
    fn menu(&self) -> Vec<MenuItem<Self>> {
        let s = &self.snapshot;
        let c = &self.controller;
        vec![
            StandardItem {
                label: s.phase.title(c.language).into(),
                enabled: false,
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            self.item(
                "Open Moosuri",
                "무수리 열기",
                Action::Chat,
                s.phase == Phase::Ready,
            ),
            self.item(
                "Web administration",
                "웹 관리자 화면",
                Action::Admin,
                s.phase == Phase::Ready,
            ),
            MenuItem::Separator,
            self.item(
                "Start server",
                "서버 시작",
                Action::Start,
                s.controllable && !s.phase.busy() && s.phase != Phase::Ready,
            ),
            self.item(
                "Stop server",
                "서버 중지",
                Action::Stop,
                s.controllable && !s.phase.busy() && s.phase != Phase::Stopped,
            ),
            CheckmarkItem {
                label: c
                    .label("Start server at login", "로그인 시 서버 자동 시작")
                    .into(),
                checked: s.autostart,
                enabled: s.controllable && !s.phase.busy(),
                activate: Box::new(|this: &mut Self| {
                    this.controller
                        .send(Action::Autostart(!this.snapshot.autostart))
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: c
                    .label(
                        "Stopping affects all users and scheduled tasks",
                        "서버 중지 시 모든 사용자·예약이 멈춥니다",
                    )
                    .into(),
                enabled: false,
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            SubMenu {
                label: c.label("Diagnostics", "진단 보기").into(),
                submenu: vec![
                    StandardItem {
                        label: c.detail(s).into(),
                        enabled: false,
                        ..Default::default()
                    }
                    .into(),
                    StandardItem {
                        label: format!("Moosuri {}", env!("CARGO_PKG_VERSION")),
                        enabled: false,
                        ..Default::default()
                    }
                    .into(),
                    self.item(
                        "Installation and troubleshooting",
                        "설치 안내와 문제 해결",
                        Action::Help,
                        true,
                    ),
                ],
                ..Default::default()
            }
            .into(),
            self.item(
                "Quit menu app (keep server running)",
                "메뉴 앱 종료 (서버는 계속 실행)",
                Action::Quit,
                true,
            ),
        ]
    }
}
pub(super) fn run(c: Controller) -> Result<()> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(error)?;
    rt.block_on(async {
        let mut previous = c.snapshot();
        let tray = Tray {
            snapshot: previous.clone(),
            icon: icon_rgba(),
            controller: c.clone(),
        };
        // A login session may start this before the panel's watcher appears.
        let handle = tray
            .assume_sni_available(true)
            .spawn()
            .await
            .map_err(|_| "desktop_status_notifier_unavailable".to_string())?;
        loop {
            let snapshot = c.snapshot();
            if snapshot.exit || handle.is_closed() {
                break;
            }
            if snapshot != previous {
                previous = snapshot.clone();
                if handle
                    .update(|tray| tray.snapshot = snapshot)
                    .await
                    .is_none()
                {
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        handle.shutdown().await;
        Ok(())
    })
}
