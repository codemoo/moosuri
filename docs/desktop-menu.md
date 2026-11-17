# Desktop menu / 데스크톱 메뉴

Moosuri's small desktop menu controls your installed **server**. Chat, model and
account settings, invitations, connections and usage remain in the web dashboard.
The same signed Rust binary provides both; no webview or second settings app is
installed. Windows WSL2 uses a Linux service and web shortcut, without a Windows
native tray.

- **Open Moosuri / 무수리 열기:** opens chat in your browser when ready. When stopped, **Start and open Moosuri / 시작하고 무수리 열기** starts the owned user service and then opens chat.
- **Web administration / 웹 관리자 화면:** opens `/admin` when ready; normal sign-in and roles apply.
- **Start/stop/restart server / 서버 시작·중지·재시작:** affects every member and scheduled task on this installation.
- **Start server at login / 로그인 시 서버 자동 시작:** changes the login policy without stopping a running server. Manual start/stop preserves this choice.
- **Diagnostics / 진단 보기:** basic readiness, a concise last-action problem, version and installation guide. No personal data or raw service logs are copied into the menu.
- **Quit menu app / 메뉴 앱 종료:** closes only the menu. The server keeps running.

설치기에서 데스크톱 바로가기를 추가하면 Mac 메뉴 막대 또는 Linux 트레이에서
상태를 보고 서버를 켜고 끌 수 있습니다. **서버 중지는 가족·친구와 예약 작업에도
적용**됩니다. 개인 비서의 일시정지와는 다릅니다. 브라우저나 메뉴 앱을 닫아도
서버는 계속 실행됩니다. 메뉴는 로그인 시 다시 열리고, 서버 자동 시작 여부는
메뉴의 체크 항목에서 따로 선택합니다.

The installer offers desktop integration in a graphical session with a usable user
service. SSH/headless installations keep their existing service/foreground flow.
macOS installs an owner-managed `~/Applications/Moosuri.app` shortcut and separate
menu LaunchAgent. Linux installs a Desktop Entry and session autostart entry.
WSL2 receives only the web entry, usable where its desktop integration is available;
`moosuri open --start` and the printed browser URL remain available otherwise. The WSL shortcut starts the service if needed; without a user service it opens a terminal that hosts the foreground server. Keep that terminal open, and use Ctrl+C to stop.

Linux needs a StatusNotifier/AppIndicator host. KDE and compatible panels provide
one; GNOME may need the distribution's AppIndicator extension. A running D-Bus menu
without a supporting panel does not mean the server failed. The CLI still works.
User-service lifetime after logout, machine sleep and WSL shutdown follows the
[installation guide](install.md); a tray icon does not keep a computer awake.

```sh
moosuri open --start     # Start if needed and open chat / 시작 후 채팅 열기
moosuri restart          # Restart, preserving autostart / 자동 시작 설정 유지하며 재시작
moosuri desktop          # Reopen the menu / 메뉴 다시 열기
moosuri autostart on     # Enable server at login / 서버 자동 시작 켜기
moosuri autostart off    # Disable login startup, keep current server / 자동 시작만 끄기
moosuri status
```

Rerun `moosuri setup` to add desktop access to the same current-version installation.
Existing older installations first follow the [signed update procedure](local-installation.md).
A newer bootstrap does not silently replace an older server or erase its accounts.
Only unchanged owned shortcuts are removed by uninstall, and private data remains.

Web entries wait during a lifecycle operation. Chat can start a stopped owned service; administration requires readiness. Readiness is rechecked on activation and includes this installation’s runtime ID, so a different server on the same port is reported as a conflict and never opened. Diagnostics retain a safe error category and useful next
command until another control/web action succeeds (opening help does not clear it).
Quitting is not lost behind an occupied action queue. An already-started lifecycle
operation finishes within its bounded service/probe limits; quitting does not
cancel that operation or stop the server. If the menu exits immediately during
setup, setup explains how to reopen it and continues with the web/CLI fallback.

The menu also survives the installation terminal closing. Server stop/removal wait
for the installed runtime to release its admission lock after the OS acknowledges
shutdown; if it takes more than 20 seconds, diagnostics report pending shutdown
instead of claiming completion.
