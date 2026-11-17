//! Fixed, local recovery guidance. Never echo service output, paths or credentials in a menu.
use super::Language;

pub(super) fn classify(error: &str) -> &'static str {
    match error {
        "setup_control_busy" | "installation_runtime_busy" => "busy",
        "setup_no_user_service_run_moosuri_start_foreground" | "setup_no_user_service" => {
            "foreground"
        }
        "setup_foreground_stop_use_ctrl_c" => "foreground_stop",
        "setup_service_not_ready_check_moosuri_status" => "not_ready",
        "setup_port_occupied_by_another_server" => "port_conflict",
        "setup_service_stop_pending_check_moosuri_status" => "stopping",
        "setup_browser_unavailable_open_printed_url" => "browser",
        "setup_service_start_failed_run_foreground"
        | "setup_service_stop_failed"
        | "setup_autostart_failed" => "service",
        "setup_existing_version_requires_signed_update" => "update",
        "setup_pending_uninstall_resume_with_manager_uninstall" | "setup_uninstall_pending" => {
            "removing"
        }
        "installation_activation_pending_run_recover" => "recovery",
        e if e.contains("file_changed")
            || e.contains("file_preserved")
            || e == "setup_existing_service_preserved"
            || e == "installation_prefix_exists_preserved" =>
        {
            "preserved"
        }
        e if e.starts_with("installation_")
            && (e.contains("signature")
                || e.contains("hash_mismatch")
                || e.contains("invalid_metadata")) =>
        {
            "integrity"
        }
        e if e.starts_with("tailscale_") => "tailscale",
        _ => "unavailable",
    }
}
pub(super) fn hint(code: &str, language: Language) -> &'static str {
    let (en, ko) = match code {
        "tailscale" => (
            "Run moosuri tailscale status. Resume interrupted connection with connect or restore with disconnect.",
            "moosuri tailscale status로 확인하세요. connect로 연결을 재개하거나 disconnect로 이전 접속을 복구하세요.",
        ),
        "busy" => (
            "Another setup or control action is in progress. Finish it, then retry.",
            "설치 또는 제어 명령이 진행 중입니다. 마친 뒤 다시 실행하세요.",
        ),
        "foreground" => (
            "No user service. Run moosuri start --foreground in a terminal.",
            "사용자 서비스가 없습니다. 터미널에서 moosuri start --foreground로 실행하세요.",
        ),
        "foreground_stop" => (
            "Stop the foreground server with Ctrl+C in its terminal.",
            "실행 중인 터미널에서 Ctrl+C로 서버를 종료하세요.",
        ),
        "not_ready" => (
            "Web is not ready. Run moosuri status; start the server if stopped.",
            "웹이 준비되지 않았습니다. moosuri status로 확인하고 정지 상태라면 시작하세요.",
        ),
        "port_conflict" => (
            "Another server is using this port. Close that server or review the configured port; Moosuri did not open it.",
            "다른 서버가 이 포트를 사용 중입니다. 해당 서버를 종료하거나 포트 설정을 확인하세요. 다른 서버의 웹은 열지 않았습니다.",
        ),
        "stopping" => (
            "Shutdown is still finishing. Run moosuri status before retrying removal.",
            "서버가 종료를 마무리하고 있습니다. moosuri status로 확인한 뒤 제거를 다시 실행하세요.",
        ),
        "browser" => (
            "No browser opener. Open the URL printed by moosuri status.",
            "브라우저를 열 수 없습니다. moosuri status에 표시된 주소로 접속하세요.",
        ),
        "service" => (
            "User service failed. Run moosuri status for its log location.",
            "사용자 서비스 명령이 실패했습니다. moosuri status에서 로그 위치를 확인하세요.",
        ),
        "update" => (
            "An older version is installed. Use the signed update guide; repeating setup will not upgrade it.",
            "이전 버전이 설치되어 있습니다. 서명된 업데이트 안내를 따르세요. 설치 재실행으로는 업그레이드되지 않습니다.",
        ),
        "removing" => (
            "Removal was interrupted. Run moosuri uninstall to finish; private data stays.",
            "제거가 중단되었습니다. moosuri uninstall로 마무리하세요. 개인 데이터는 유지합니다.",
        ),
        "recovery" => (
            "An update is incomplete. Follow signed-update recovery before starting.",
            "업데이트가 끝나지 않았습니다. 시작 전에 서명된 업데이트 복구 안내를 따르세요.",
        ),
        "preserved" => (
            "Existing files differ. They were preserved; check the installation guide before retrying.",
            "기존 파일이 달라 보존했습니다. 재시도 전에 설치 안내를 확인하세요.",
        ),
        "integrity" => (
            "Installation verification failed. Use a verified signed bundle; keep your data.",
            "설치 무결성 검증이 실패했습니다. 데이터를 유지하고 검증된 서명 패키지를 사용하세요.",
        ),
        _ => (
            "Check the details and installation guide. Use moosuri status for an installed server.",
            "상세 코드와 설치 안내를 확인하세요. 설치된 서버는 moosuri status로 확인할 수 있습니다.",
        ),
    };
    language.text(en, ko)
}
