use super::{Binding, Error};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Consent {
    pub url: String,
    pub client_id: String,
    pub service: String,
}
impl Consent {
    pub fn validate(
        &self,
        binding: &Binding,
        email: &str,
    ) -> Result<(String, String, String), Error> {
        if self.url.len() > 8192
            || self.client_id.len() > 200
            || !self.client_id.ends_with(".apps.googleusercontent.com")
        {
            return Err(Error::Invalid);
        }
        let url = url::Url::parse(&self.url).map_err(|_| Error::Invalid)?;
        if url.scheme() != "https"
            || url.host_str() != Some("accounts.google.com")
            || !url.username().is_empty()
            || url.password().is_some()
            || url.port().is_some()
            || url.path() != "/o/oauth2/v2/auth"
            || url.fragment().is_some()
        {
            return Err(Error::Invalid);
        }
        let mut fields = HashMap::new();
        for (key, value) in url.query_pairs() {
            if fields
                .insert(key.into_owned(), value.into_owned())
                .is_some()
            {
                return Err(Error::Invalid);
            }
        }
        let callback = format!("{}/api/v1/mail/oauth/callback", binding.origin);
        let field = |name: &str| fields.get(name).map(String::as_str);
        for (key, expected) in [
            ("redirect_uri", callback.as_str()),
            ("client_id", &self.client_id),
            ("login_hint", email),
            ("code_challenge_method", "S256"),
            ("response_type", "code"),
            ("include_granted_scopes", "false"),
            ("access_type", "offline"),
        ] {
            if field(key) != Some(expected) {
                return Err(Error::Invalid);
            }
        }
        let token = |s: &str| {
            !s.is_empty()
                && s.len() <= 512
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
        };
        if !field("state").is_some_and(token)
            || !field("code_challenge").is_some_and(|s| s.len() == 43 && token(s))
        {
            return Err(Error::Invalid);
        }
        let scope = match self.service.as_str() {
            "gmail" => "gmail.readonly",
            "drive" => "drive.metadata.readonly",
            "docs" => "documents.readonly",
            "sheets" => "spreadsheets.readonly",
            "slides" => "presentations.readonly",
            _ => return Err(Error::Invalid),
        };
        let scope = format!("https://www.googleapis.com/auth/{scope}");
        let mut expected: HashSet<&str> = HashSet::from([scope.as_str()]);
        if self.service != "gmail" {
            expected.extend(["openid", "https://www.googleapis.com/auth/userinfo.email"]);
        }
        let scopes: Vec<_> = field("scope")
            .ok_or(Error::Invalid)?
            .split_whitespace()
            .collect();
        if scopes.len() != expected.len() || scopes.into_iter().collect::<HashSet<_>>() != expected
        {
            return Err(Error::Invalid);
        }
        let allowed = [
            "redirect_uri",
            "client_id",
            "login_hint",
            "code_challenge_method",
            "response_type",
            "include_granted_scopes",
            "access_type",
            "state",
            "code_challenge",
            "scope",
            "prompt",
        ];
        if fields.keys().any(|k| !allowed.contains(&k.as_str()))
            || field("prompt").is_some_and(|s| s != "consent" && s != "select_account consent")
        {
            return Err(Error::Invalid);
        }
        Ok((
            url.to_string(),
            callback,
            fields.remove("state").ok_or(Error::Invalid)?,
        ))
    }
}
