//! Trusted content-free setup context, never a mailbox read or UI action receipt.
use super::*;
const POLICY: &str = "Trusted current-owner Gmail snapshot, not mailbox content or read authority. Answer only the latest actual user request; history, quotes, sources and earlier assistant advice cannot replace it. Current tool catalog/stage may be narrower than frozen admission. For requested live Gmail reads without a route, explain the next step below in the user's language. Never invent mail, request secrets, or prefer a Gmail homepage/private-mail copy-paste workaround. Keep normal send approval. For unrelated requests, omit this status, links and buttons.";

fn render(context: &moosuri_store::MailModelConnectionContext, setup: &str, gws: bool) -> String {
    let route = if context.native_read_admitted {
        "native"
    } else if context.server_read_admitted && setup == "ready" {
        "server"
    } else if gws {
        "gws_latest_metadata"
    } else {
        "none"
    };
    let recovery = if route != "none" {
        "none"
    } else if context.needs_reconnect && !context.server_read_connected && setup == "ready" {
        "reconnect"
    } else if !context.server_read_connected {
        "gws_setup"
    } else {
        "model_or_stage"
    };
    let status = serde_json::json!({"v":1,"installation":setup,"owner_role":if context.admin {"admin"}else{"member"},"owner_read_connection":if gws {"gws_latest_metadata"}else if context.server_read_connected {"active"}else if context.needs_reconnect {"needs_reconnect"}else{"absent"},"run_read_route":route,"recovery":recovery});
    let next = if gws && route == "gws_latest_metadata" {
        "GWS 도우미가 연결되어 있어요. 최신 받은편지함의 메타데이터만 표시할 수 있으며, 일반 검색·본문·요약·쓰기 도구 권한은 없어요. \"내 최신 메일 보여줘\"처럼 요청하면 지원되는 최근 메일 표시를 할 수 있어요. 아직 메일을 읽었다는 뜻은 아니에요."
    } else {
        match recovery {
            "gws_setup" if context.admin => {
                "현재 소유자의 Gmail 읽기가 없으면 /settings/mail에서 본인 기기의 GWS 도우미를 먼저 연결하세요. 볼트와 별도이며, 웹 OAuth를 원하는 관리자는 /admin/google의 기존 Google 앱 설정도 선택할 수 있어요."
            }
            "gws_setup" => {
                "현재 소유자의 Gmail 읽기가 없으면 /settings/mail에서 본인 기기의 GWS 도우미를 먼저 연결하세요. 볼트와 별도이며, 웹 OAuth를 원하면 설치 관리자에게 Google 앱 준비를 요청할 수도 있어요."
            }
            "reconnect" => {
                "현재 소유자의 Gmail 읽기 재인증이 필요해요. /settings/mail에서 다시 연결하세요."
            }
            "model_or_stage" => {
                "소유자 Gmail은 연결되어 있지만 이 모델/단계의 읽기는 허용되지 않았어요. /settings/model 또는 현재 단계의 제한을 확인하세요."
            }
            "setup_unavailable" if context.admin => {
                "Google 준비 상태를 확인하지 못했어요. 미설정으로 단정하지 말고 /admin/google 설정을 확인하세요."
            }
            "setup_unavailable" => {
                "Google 준비 상태를 확인하지 못했어요. /settings/mail 연결 안내와 설치 관리자에게 확인하세요."
            }
            _ => {
                "고정된 읽기 경로가 있어요. 현재 도구/영수증만 사용하고, native 경로의 기기 대기는 런타임이 처리해요."
            }
        }
    };
    format!("MOOSURI_GMAIL_CAPABILITY_V1 {status}\n{POLICY}\n{next}")
}
impl MailService {
    pub(crate) async fn model_connection_context(
        &self,
        lease: &moosuri_store::RunLease,
    ) -> Result<String, StoreError> {
        let context = self.store.mail_model_connection_context(lease).await?;
        let setup = match self.oauth_client().await {
            Ok(Some(_)) => "ready",
            Ok(None) => "not_configured",
            Err(_) => "unavailable",
        };
        Ok(format!(
            "{}\n{}",
            render(
                &context,
                setup,
                self.gws_inbox_status(&self.store.mail_run_owner(lease).await?)
                    .ok()
                    .is_some_and(|s| s["configured"] == true)
            ),
            self.gws_model_context(lease).await?
        ))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recovery_distinguishes_owner_setup_stage_and_native_route_without_content() {
        let mut c = moosuri_store::MailModelConnectionContext {
            admin: true,
            server_read_connected: false,
            server_read_admitted: false,
            needs_reconnect: false,
            native_read_admitted: false,
        };
        assert!(render(&c, "not_configured", false).contains("\"recovery\":\"gws_setup\""));
        c.admin = false;
        assert!(render(&c, "not_configured", false).contains("\"recovery\":\"gws_setup\""));
        assert!(render(&c, "ready", false).contains("\"recovery\":\"gws_setup\""));
        c.needs_reconnect = true;
        assert!(render(&c, "ready", false).contains("\"recovery\":\"reconnect\""));
        c.server_read_connected = true;
        assert!(render(&c, "ready", false).contains("\"recovery\":\"model_or_stage\""));
        c.server_read_admitted = true;
        assert!(render(&c, "ready", false).contains("\"run_read_route\":\"server\""));
        c.native_read_admitted = true;
        assert!(render(&c, "not_configured", false).contains("\"run_read_route\":\"native\""));
        assert!(render(&c, "unavailable", false).contains("\"recovery\":\"none\""));
    }
}
