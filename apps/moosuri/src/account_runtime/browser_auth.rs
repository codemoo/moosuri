//! Authentication-page guard for the member browser.
//!
//! The general workspace reads public pages only. When a public request lands on a sign-in,
//! OAuth, human-verification or blocked page (directly or through redirects), the page is
//! replaced by an origin-only marker before it reaches the model, the worklog or the store.
//! Nothing here signs in. Only the member's explicit, typed Google sign-in request (relay protocol in
//! `browser_relay::google`) may attempt it, and success exists only as the bridge's typed `ready/verified`.
use crate::browser_relay::google::{Outcome, Reason, Status};
use url::Url;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Auth {
    SignIn,
    GoogleSignIn,
    HumanVerification,
    Blocked,
}
pub(super) const REASON_PREFIX: &str = "browser_auth_";

impl Auth {
    pub(super) fn reason(self) -> &'static str {
        match self {
            Self::SignIn => "browser_auth_required",
            Self::GoogleSignIn => "browser_auth_google",
            Self::HumanVerification => "browser_auth_human_verification",
            Self::Blocked => "browser_auth_blocked",
        }
    }
    /// Fixed, truthful text. It never echoes page content, URLs or form metadata.
    pub(super) fn note(self) -> &'static str {
        match self {
            Self::SignIn => {
                "로그인이 필요한 화면으로 이동해 읽기를 멈췄습니다. 화면 내용과 주소는 저장하거나 모델에 전달하지 않았습니다. 이 작업공간에서는 비밀번호나 인증 코드를 입력하거나 대신 조작하지 않습니다. 공개 페이지로 다시 요청해 주세요."
            }
            Self::GoogleSignIn => {
                "Google 로그인 화면에 도착해 읽기를 멈췄습니다. Google 로그인은 이 작업공간에서 진행하거나 확인하지 않았고 로그인된 상태가 아닙니다. 본인 기기에서 Google 공식 페이지(accounts.google.com)를 직접 열거나, 내 Google Workspace 연결 동의를 사용하세요."
            }
            Self::HumanVerification => {
                "사람 확인(보안문자 또는 추가 인증)이 필요해 멈췄습니다. 자동으로 통과하거나 우회하지 않으며 로그인되지 않았습니다. 본인 기기에서 직접 진행해 주세요."
            }
            Self::Blocked => {
                "사이트가 이 브라우저의 로그인을 거부하거나 차단했습니다. 로그인되지 않았습니다. 본인 기기에서 공식 페이지로 직접 진행해 주세요."
            }
        }
    }
}
pub(super) fn is_reason(reason: &str) -> bool {
    reason.starts_with(REASON_PREFIX)
}
/// Only the exact official Google account origin is trusted; lookalike hosts are not.
pub(super) fn is_official_google(url: &Url) -> bool {
    url.scheme() == "https"
        && url.host_str() == Some("accounts.google.com")
        && url.port().is_none()
        && url.username().is_empty()
        && url.password().is_none()
}
pub(super) fn origin_only(raw: &str) -> String {
    match Url::parse(raw) {
        Ok(url) if matches!(url.scheme(), "http" | "https") && url.host_str().is_some() => {
            url.origin().ascii_serialization()
        }
        _ => String::new(),
    }
}
fn url_signal(url: &Url) -> bool {
    let path = url.path().to_ascii_lowercase();
    let token = path.split(|c: char| !c.is_ascii_alphanumeric()).any(|t| {
        matches!(
            t,
            "login"
                | "logon"
                | "signin"
                | "auth"
                | "oauth"
                | "oauth2"
                | "sso"
                | "saml"
                | "password"
                | "passwd"
                | "authorize"
                | "servicelogin"
                | "accountchooser"
                | "challenge"
        )
    }) || path.contains("sign-in")
        || path.contains("log-in");
    let host = url
        .host_str()
        .and_then(|h| h.split('.').next())
        .is_some_and(|label| {
            matches!(
                label,
                "login" | "auth" | "sso" | "signin" | "accounts" | "oauth" | "idp"
            )
        });
    let query = url.query_pairs().any(|(k, _)| {
        matches!(
            k.to_ascii_lowercase().as_str(),
            "client_id"
                | "redirect_uri"
                | "response_type"
                | "access_token"
                | "id_token"
                | "oauth_token"
                | "password"
                | "passwd"
                | "code_challenge"
                | "login_hint"
                | "samlrequest"
                | "samlresponse"
        )
    });
    token || host || query
}
const CHALLENGE: &[&str] = &[
    "captcha",
    "verify you are human",
    "verify you're human",
    "unusual traffic",
    "not a robot",
    "로봇이 아닙니다",
    "보안문자",
    "자동 입력 방지",
];
const VERIFY: &[&str] = &[
    "verify it's you",
    "2-step verification",
    "two-factor",
    "verification code",
    "본인 확인",
    "본인 인증",
    "인증 코드",
    "2단계 인증",
];
const BLOCKED: &[&str] = &[
    "couldn't sign you in",
    "can't sign you in",
    "browser or app may not be secure",
    "로그인할 수 없",
    "로그인하지 못했",
    "보안되지 않을 수",
    "보안상의 이유로 로그인",
];
fn any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|n| haystack.contains(n))
}
/// Classify a rendered page from its raw URL (query intact), title and visible text.
pub(super) fn classify(raw_url: &str, title: &str, text: &str) -> Option<Auth> {
    let url = Url::parse(raw_url).ok();
    let google = url.as_ref().is_some_and(is_official_google);
    let by_url = url.as_ref().is_some_and(url_signal);
    let body = format!("{title}\n{text}").to_lowercase();
    // Interstitials and login forms are short; long documents may merely discuss them.
    let short = body.chars().count() < 800;
    let password = ["password", "passcode", "비밀번호"]
        .iter()
        .any(|w| body.contains(w))
        && any(
            &body,
            &[
                "sign in",
                "log in",
                "login",
                "로그인",
                "next",
                "다음",
                "forgot",
            ],
        );
    if google || by_url || (short && password) {
        return Some(if any(&body, BLOCKED) {
            Auth::Blocked
        } else if any(&body, CHALLENGE) || any(&body, VERIFY) {
            Auth::HumanVerification
        } else if google {
            Auth::GoogleSignIn
        } else {
            Auth::SignIn
        });
    }
    (short && any(&body, CHALLENGE)).then_some(Auth::HumanVerification)
}
/// Classify and, on a hit, wipe everything but the origin so no login evidence survives.
pub(super) fn guard(url: &mut String, title: &mut String, text: &mut String) -> Option<Auth> {
    let found = classify(url, title, text)?;
    *url = origin_only(url);
    title.clear();
    text.clear();
    Some(found)
}
/// True when the address alone (query intact) identifies an authentication page.
pub(super) fn is_auth_url(raw: &str) -> bool {
    classify(raw, "", "").is_some()
}

/// Work-record state for a typed Google sign-in attempt (distinct from `browser_auth_*` page stops).
pub(super) const GOOGLE_PREFIX: &str = "browser_google_";
pub(super) const GOOGLE_UNKNOWN: &str = "browser_google_unknown";
pub(super) fn is_google_reason(reason: &str) -> bool {
    reason.starts_with(GOOGLE_PREFIX)
}
/// The member-facing official link: opened by the member on their own device, never proof of success.

/// What the record shows for one attempt. `None` means the answer never arrived: uncertain, never retried.
pub(super) struct GoogleView {
    pub(super) state: &'static str,
    pub(super) reason: &'static str,
    pub(super) note: String,
}
/// Success is only the bridge's typed `ready/verified`; every other answer says plainly that nobody is signed in.
pub(super) fn google_view(outcome: Option<Outcome>) -> GoogleView {
    use Reason::*;
    let uncertain = || {
        GoogleView {
        state: "outcome_unknown",
        reason: GOOGLE_UNKNOWN,
        note: "Google 로그인 결과를 확인하지 못했습니다. 로그인되었다고 표시하지 않으며 자동으로 다시 시도하지 않습니다. 필요하면 새로 요청해 주세요.".into(),
    }
    };
    let Some(outcome) = outcome else {
        return uncertain();
    };
    let (reason, note) = match (outcome.status, outcome.reason) {
        (Status::Ready, Verified) => (
            "browser_google_ready",
            "Kiri Chrome에서 지정된 Google 계정 로그인을 확인했습니다. 비밀번호나 인증 코드는 채팅과 모델에 전달되지 않았습니다.".to_owned(),
        ),
        (Status::NeedsHuman, SignInRequired) => (
            "browser_google_needs_human",
            "Kiri의 Google 로그인 화면에서 추가 입력을 요청해 로그인되지 않았습니다. 저장된 계정을 사용하려면 Kiri에서 무수리 회원 볼트를 열어 주세요.".to_owned(),
        ),
        (Status::NeedsHuman, HumanVerification) => (
            "browser_google_needs_human",
            "Google이 Kiri 로그인에 본인 확인을 요청했습니다. 이번 시도에서는 로그인되지 않았습니다.".to_owned(),
        ),
        (Status::Blocked, _) => (
            "browser_google_blocked",
            "Google이 Kiri 로그인을 거절했거나 계정이 일치하지 않아 로그인되지 않았습니다.".to_owned(),
        ),
        (Status::Unavailable, Timeout | BridgeError | AlreadyAttempted) => return uncertain(),
        (Status::Unavailable, reason) => (
            "browser_google_unavailable",
            match reason {
                NotConfigured => "이 설치에는 Google 로그인 연결이 구성되지 않았습니다.",
                NotEnrolled => "이 계정은 Google 로그인 대상으로 등록되어 있지 않습니다.",
                _ => "다른 Google 로그인 시도가 진행 중입니다. 잠시 후 다시 요청해 주세요.",
            }
            .to_owned()
                + " 로그인되지 않았습니다.",
        ),
        _ => return uncertain(),
    };
    GoogleView {
        state: "completed",
        reason,
        note,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_public_pages_are_untouched() {
        assert_eq!(
            classify(
                "https://example.org/timetable?x=1",
                "Timetable",
                "Morning bus 08:15"
            ),
            None
        );
        let long = format!(
            "{} captcha and password login discussion",
            "word ".repeat(300)
        );
        assert_eq!(
            classify("https://example.org/security", "Guide", &long),
            None
        );
        assert!(!is_auth_url("https://example.org/detail"));
    }
    #[test]
    fn redirect_to_google_sign_in_is_wiped() {
        let mut url = "https://accounts.google.com/v3/signin/identifier?continue=https%3A%2F%2Fexample.org&login_hint=CANARY@example.test".to_owned();
        let mut title = "Sign in - Google Accounts CANARY".to_owned();
        let mut text = "Email or phone CANARY_FORM_FIELD".to_owned();
        assert_eq!(
            guard(&mut url, &mut title, &mut text),
            Some(Auth::GoogleSignIn)
        );
        assert_eq!(url, "https://accounts.google.com");
        assert!(title.is_empty() && text.is_empty());
    }
    #[test]
    fn oauth_redirect_on_other_host_is_generic_sign_in() {
        let raw =
            "https://idp.example.net/start?client_id=abc&redirect_uri=https%3A%2F%2Fx&state=CANARY";
        assert_eq!(classify(raw, "", ""), Some(Auth::SignIn));
        assert_eq!(origin_only(raw), "https://idp.example.net");
        assert_eq!(
            classify("https://example.org/members/login#frag", "", ""),
            Some(Auth::SignIn)
        );
    }
    #[test]
    fn only_exact_google_origin_is_trusted() {
        for bad in [
            "https://accounts.google.com.evil.test/signin",
            "https://accounts-google.com/signin",
            "http://accounts.google.com/signin",
            "https://user@accounts.google.com/signin",
            "https://accounts.google.com:8443/signin",
        ] {
            let url = Url::parse(bad).unwrap();
            assert!(!is_official_google(&url), "{bad}");
        }
        assert_eq!(
            classify("https://accounts.google.com.evil.test/signin", "", ""),
            Some(Auth::SignIn)
        );
    }
    #[test]
    fn human_verification_and_block_are_explicit() {
        assert_eq!(
            classify(
                "https://accounts.google.com/signin/v2/challenge/recaptcha",
                "",
                "Confirm you're not a robot"
            ),
            Some(Auth::HumanVerification)
        );
        assert_eq!(
            classify(
                "https://example.org/",
                "Just a moment",
                "Verify you are human"
            ),
            Some(Auth::HumanVerification)
        );
        assert_eq!(
            classify(
                "https://accounts.google.com/signin",
                "",
                "Couldn't sign you in. This browser or app may not be secure."
            ),
            Some(Auth::Blocked)
        );
        assert!(
            Auth::HumanVerification
                .note()
                .contains("로그인되지 않았습니다")
        );
        assert!(Auth::Blocked.note().contains("로그인되지 않았습니다"));
    }
    #[test]
    fn only_typed_ready_is_success_and_uncertain_is_never_retried() {
        let o = |status, reason| {
            Some(Outcome {
                v: 1,
                status,
                reason,
            })
        };
        let ready = google_view(o(Status::Ready, Reason::Verified));
        assert_eq!(
            (ready.state, ready.reason),
            ("completed", "browser_google_ready")
        );
        for (status, reason, expected) in [
            (
                Status::NeedsHuman,
                Reason::HumanVerification,
                "browser_google_needs_human",
            ),
            (
                Status::Blocked,
                Reason::AccountMismatch,
                "browser_google_blocked",
            ),
            (
                Status::Unavailable,
                Reason::NotConfigured,
                "browser_google_unavailable",
            ),
        ] {
            let view = google_view(o(status, reason));
            assert_eq!((view.state, view.reason), ("completed", expected));
            assert!(view.note.contains("로그인되지 않았습니다"), "{expected}");
            assert!(is_google_reason(view.reason));
        }
        assert!(
            google_view(o(Status::NeedsHuman, Reason::SignInRequired))
                .note
                .contains("Kiri에서 무수리 회원 볼트")
        );
        for uncertain in [
            None,
            o(Status::Unavailable, Reason::Timeout),
            o(Status::Unavailable, Reason::AlreadyAttempted),
            o(Status::Ready, Reason::SignInRequired),
        ] {
            let view = google_view(uncertain);
            assert_eq!(
                (view.state, view.reason),
                ("outcome_unknown", GOOGLE_UNKNOWN)
            );
            assert!(!view.note.contains("확인했습니다"));
        }
    }
    #[test]
    fn notes_and_reasons_leak_nothing_and_are_prefixed() {
        for a in [
            Auth::SignIn,
            Auth::GoogleSignIn,
            Auth::HumanVerification,
            Auth::Blocked,
        ] {
            assert!(is_reason(a.reason()));
            assert!(!a.note().contains("http") && !a.note().contains('@'));
        }
        assert!(
            Auth::GoogleSignIn
                .note()
                .contains("로그인된 상태가 아닙니다")
        );
    }
}
