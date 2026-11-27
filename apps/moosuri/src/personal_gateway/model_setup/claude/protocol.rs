//! Official login helper protocol: bounded JSON lines on stdout. At most one
//! `authorization_url`, then exactly one `result`, then end of output.
use serde::Deserialize;

pub(super) const MAX_LINE: usize = 8192;
pub(super) const MAX_LINES: usize = 4;
const URL_PREFIX: &str = "https://claude.com/cai/oauth/authorize?";

#[derive(Deserialize)]
#[serde(tag = "event", rename_all = "snake_case", deny_unknown_fields)]
enum Wire {
    AuthorizationUrl { url: String },
    Result { result: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Outcome {
    Ready,
    Cancelled,
    Timeout,
    Failed,
}
pub(super) enum Step {
    Url(String),
    Result(Outcome),
}

/// Exact official origin and path only: no credentials, port, fragment or
/// alternate spelling that parses to the same URL.
pub(super) fn official_url(url: &str) -> bool {
    if url.len() > 4096
        || !url.starts_with(URL_PREFIX)
        || url.len() == URL_PREFIX.len()
        || !url.bytes().all(|b| b.is_ascii_graphic())
    {
        return false;
    }
    url::Url::parse(url).is_ok_and(|u| {
        u.scheme() == "https"
            && u.host_str() == Some("claude.com")
            && u.username().is_empty()
            && u.password().is_none()
            && u.port().is_none()
            && u.path() == "/cai/oauth/authorize"
            && u.fragment().is_none()
    })
}
/// A one-use authorization code: 8..=4096 visible ASCII characters.
pub(super) fn valid_code(code: &str) -> bool {
    (8..=4096).contains(&code.len()) && code.bytes().all(|b| b.is_ascii_graphic())
}

#[derive(Default)]
pub(super) struct Sequence {
    url: bool,
    result: Option<Outcome>,
}
impl Sequence {
    pub(super) fn accept(&mut self, line: &[u8]) -> Result<Step, ()> {
        // Anything after the result is a protocol violation, never a success.
        if self.result.is_some() || line.len() > MAX_LINE {
            return Err(());
        }
        match serde_json::from_slice::<Wire>(line).map_err(|_| ())? {
            Wire::AuthorizationUrl { url } => {
                if self.url || !official_url(&url) {
                    return Err(());
                }
                self.url = true;
                Ok(Step::Url(url))
            }
            Wire::Result { result } => {
                let outcome = match result.as_str() {
                    "ready" => Outcome::Ready,
                    "cancelled" => Outcome::Cancelled,
                    "timeout" => Outcome::Timeout,
                    _ => Outcome::Failed,
                };
                self.result = Some(outcome);
                Ok(Step::Result(outcome))
            }
        }
    }
    pub(super) fn finished(&self) -> Option<Outcome> {
        self.result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const URL: &str = r#"{"event":"authorization_url","url":"https://claude.com/cai/oauth/authorize?code=true&state=s"}"#;
    #[test]
    fn only_the_exact_official_authorization_url_is_accepted() {
        assert!(official_url("https://claude.com/cai/oauth/authorize?a=b"));
        for bad in [
            "http://claude.com/cai/oauth/authorize?a=b",
            "https://claude.com.evil.test/cai/oauth/authorize?a=b",
            "https://user@claude.com/cai/oauth/authorize?a=b",
            "https://claude.com:443/cai/oauth/authorize?a=b",
            "https://claude.com:8443/cai/oauth/authorize?a=b",
            "https://claude.com/cai/oauth/authorize/x?a=b",
            "https://claude.com/cai/oauth/authorize?a=b#frag",
            "https://claude.com/cai/oauth/authorize",
            "https://claude.com/cai/oauth/authorize?a=b c",
            "https://claude.ai/cai/oauth/authorize?a=b",
        ] {
            assert!(!official_url(bad), "{bad}");
        }
        assert!(valid_code("abcdefgh") && !valid_code("short") && !valid_code("has space 1"));
        assert!(!valid_code(&"a".repeat(4097)));
    }
    #[test]
    fn url_at_most_once_then_exactly_one_result_and_nothing_after() {
        let mut s = Sequence::default();
        assert!(matches!(s.accept(URL.as_bytes()), Ok(Step::Url(_))));
        assert!(s.accept(URL.as_bytes()).is_err());
        let mut s = Sequence::default();
        assert!(matches!(
            s.accept(br#"{"event":"result","result":"ready"}"#),
            Ok(Step::Result(Outcome::Ready))
        ));
        assert_eq!(s.finished(), Some(Outcome::Ready));
        assert!(s.accept(URL.as_bytes()).is_err());
        assert!(s.accept(br#"{"event":"result","result":"ready"}"#).is_err());
    }
    #[test]
    fn unknown_fields_events_and_results_never_become_ready() {
        let mut s = Sequence::default();
        for bad in [
            &br#"{"event":"result","result":"ready","extra":1}"#[..],
            br#"{"event":"progress"}"#,
            b"not json",
            br#"{"event":"authorization_url","url":"https://evil.test/x?y"}"#,
        ] {
            assert!(s.accept(bad).is_err());
        }
        assert!(matches!(
            s.accept(br#"{"event":"result","result":"surprise"}"#),
            Ok(Step::Result(Outcome::Failed))
        ));
    }
}
