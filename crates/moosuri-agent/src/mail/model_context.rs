//! Trusted content-free setup context, never a mailbox read or UI action receipt.
use super::*;
const POLICY: &str = "Trusted current-owner Gmail snapshot, not mailbox content or read authority. Answer only the latest actual user request; history, quotes, sources and earlier assistant advice cannot replace it. Current tool catalog/stage may be narrower than frozen admission. For requested live Gmail reads without a route, explain the next step below in the user's language. Never invent mail, request secrets, or prefer a Gmail homepage/private-mail copy-paste workaround. Keep normal send approval. For unrelated requests, omit this status, links and buttons.";

fn render(context: &moosuri_store::MailModelConnectionContext, setup: &str) -> String {
    let route = if context.native_read_admitted {
        "native"
    } else if context.server_read_admitted && setup == "ready" {
        "server"
    } else {
        "none"
    };
    let recovery = if route != "none" {
        "none"
    } else if setup == "unavailable" {
        "setup_unavailable"
    } else if setup == "not_configured" {
        if context.admin {
            "admin_setup"
        } else {
            "member_setup"
        }
    } else if context.needs_reconnect && !context.server_read_connected {
        "reconnect"
    } else if !context.server_read_connected {
        "connect"
    } else {
        "model_or_stage"
    };
    let status = serde_json::json!({"v":1,"installation":setup,"owner_role":if context.admin {"admin"}else{"member"},"owner_read_connection":if context.server_read_connected {"active"}else if context.needs_reconnect {"needs_reconnect"}else{"absent"},"run_read_route":route,"recovery":recovery});
    let next = match recovery {
        "admin_setup" => {
            "Google 앱 미설정: 설치 관리자인 본인이 /admin/google 설정 후 /settings/mail에서 본인의 Gmail 읽기를 연결하세요."
        }
        "member_setup" => {
            "Google 앱 미설정: 설치 관리자에게 준비를 요청한 뒤 /settings/mail에서 본인 Gmail 읽기를 연결하세요."
        }
        "connect" => {
            "현재 소유자의 Gmail 읽기 연결이 없어요. /settings/mail에서 본인 계정을 연결하세요."
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
        Ok(render(&context, setup))
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
        assert!(render(&c, "not_configured").contains("\"recovery\":\"admin_setup\""));
        c.admin = false;
        assert!(render(&c, "not_configured").contains("\"recovery\":\"member_setup\""));
        assert!(render(&c, "ready").contains("\"recovery\":\"connect\""));
        c.needs_reconnect = true;
        assert!(render(&c, "ready").contains("\"recovery\":\"reconnect\""));
        c.server_read_connected = true;
        assert!(render(&c, "ready").contains("\"recovery\":\"model_or_stage\""));
        c.server_read_admitted = true;
        assert!(render(&c, "ready").contains("\"run_read_route\":\"server\""));
        c.native_read_admitted = true;
        assert!(render(&c, "not_configured").contains("\"run_read_route\":\"native\""));
        assert!(render(&c, "unavailable").contains("\"recovery\":\"none\""));
    }
}
