use super::*;
use objc2::{MainThreadMarker, rc::autoreleasepool};
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy, NSEventMask};
use objc2_foundation::{NSDate, NSDefaultRunLoopMode};
use tray_icon::{
    Icon, TrayIconBuilder,
    menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu},
};

pub(super) fn run(c: Controller) -> Result<()> {
    let mtm = MainThreadMarker::new().ok_or("desktop_main_thread_required")?;
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    let menu = Menu::new();
    let state = MenuItem::new(c.label("Checking server…", "서버 확인 중…"), false, None);
    let chat = MenuItem::new(c.label("Open Moosuri", "무수리 열기"), false, None);
    let admin = MenuItem::new(c.label("Web administration", "웹 관리자 화면"), false, None);
    let start = MenuItem::new(c.label("Start server", "서버 시작"), false, None);
    let stop = MenuItem::new(c.label("Stop server", "서버 중지"), false, None);
    let auto = CheckMenuItem::new(
        c.label("Start server at login", "로그인 시 서버 자동 시작"),
        false,
        false,
        None,
    );
    let shared = MenuItem::new(
        c.label(
            "Stopping affects all users and scheduled tasks",
            "서버 중지 시 모든 사용자·예약이 멈춥니다",
        ),
        false,
        None,
    );
    let diagnostics = Submenu::new(c.label("Diagnostics", "진단 보기"), true);
    let detail = MenuItem::new("", false, None);
    let version = MenuItem::new(
        format!("Moosuri {}", env!("CARGO_PKG_VERSION")),
        false,
        None,
    );
    let guide = MenuItem::new(
        c.label("Installation and troubleshooting", "설치 안내와 문제 해결"),
        true,
        None,
    );
    diagnostics
        .append_items(&[&detail, &version, &guide])
        .map_err(error)?;
    let quit = MenuItem::new(
        c.label(
            "Quit menu app (keep server running)",
            "메뉴 앱 종료 (서버는 계속 실행)",
        ),
        true,
        None,
    );
    menu.append_items(&[
        &state,
        &PredefinedMenuItem::separator(),
        &chat,
        &admin,
        &PredefinedMenuItem::separator(),
        &start,
        &stop,
        &auto,
        &shared,
        &PredefinedMenuItem::separator(),
        &diagnostics,
        &quit,
    ])
    .map_err(error)?;
    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_icon(Icon::from_rgba(icon_rgba(), 24, 24).map_err(error)?)
        .with_icon_as_template(false)
        .with_tooltip("Moosuri")
        .build()
        .map_err(error)?;
    app.finishLaunching();
    let mut previous = Snapshot::default();
    loop {
        let current = c.snapshot();
        if current.exit {
            break;
        }
        if current != previous {
            state.set_text(current.phase.title(c.language));
            chat.set_enabled(current.phase == Phase::Ready);
            admin.set_enabled(current.phase == Phase::Ready);
            start.set_enabled(
                current.controllable && !current.phase.busy() && current.phase != Phase::Ready,
            );
            stop.set_enabled(
                current.controllable && !current.phase.busy() && current.phase != Phase::Stopped,
            );
            auto.set_enabled(current.controllable && !current.phase.busy());
            auto.set_checked(current.autostart);
            detail.set_text(c.detail(&current));
            let _ = tray.set_tooltip(Some(format!(
                "Moosuri · {}",
                current.phase.title(c.language)
            )));
            previous = current;
        }
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            let id = event.id;
            if id == *quit.id() {
                return Ok(());
            }
            if id == *chat.id() {
                c.send(Action::Chat);
            } else if id == *admin.id() {
                c.send(Action::Admin);
            } else if id == *start.id() {
                c.send(Action::Start);
            } else if id == *stop.id() {
                c.send(Action::Stop);
            } else if id == *auto.id() {
                c.send(Action::Autostart(auto.is_checked()));
            } else if id == *guide.id() {
                c.send(Action::Help);
            }
        }
        autoreleasepool(|_| {
            let date = NSDate::dateWithTimeIntervalSinceNow(0.2);
            if let Some(event) = app.nextEventMatchingMask_untilDate_inMode_dequeue(
                NSEventMask::Any,
                Some(&date),
                unsafe { NSDefaultRunLoopMode },
                true,
            ) {
                app.sendEvent(&event);
            }
        });
    }
    Ok(())
}
