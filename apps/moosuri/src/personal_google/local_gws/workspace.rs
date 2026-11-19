use super::{Failure, Identity, OUTPUT_LIMIT, StatusCode, unavailable, valid_id};
use moosuri_connectors::gmail::{OAuthClient, SecretString, pkce_challenge};
use moosuri_store::MemberGoogleService;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{net::SocketAddr, time::Duration};
use zeroize::Zeroizing;

const EMAIL_SCOPE: &str = "https://www.googleapis.com/auth/userinfo.email";
const OPENID_SCOPE: &str = "openid";
const USERINFO: &str = "https://openidconnect.googleapis.com/v1/userinfo";
const USERINFO_LIMIT: usize = 8 * 1024;
const PAGE_SIZE: usize = 5;
const SLIDE_LIMIT: usize = 10_000;
const SHEET_LIMIT: usize = 200;

pub(in crate::personal_google) fn scopes(service: MemberGoogleService) -> [&'static str; 3] {
    [service.scope(), OPENID_SCOPE, EMAIL_SCOPE]
}

pub(super) fn authorization_url(
    app: &OAuthClient,
    service: MemberGoogleService,
    state: &str,
    verifier: &SecretString,
) -> Result<String, Failure> {
    let challenge = pkce_challenge(verifier.expose()).map_err(|_| unavailable())?;
    let mut url = url::Url::parse("https://accounts.google.com/o/oauth2/v2/auth")
        .map_err(|_| unavailable())?;
    url.query_pairs_mut().extend_pairs([
        ("client_id", app.client_id.as_str()),
        ("redirect_uri", &app.redirect_uri),
        ("response_type", "code"),
        ("scope", &scopes(service).join(" ")),
        ("include_granted_scopes", "false"),
        ("state", state),
        ("access_type", "offline"),
        ("code_challenge", &challenge),
        ("code_challenge_method", "S256"),
        ("prompt", "consent select_account"),
    ]);
    Ok(url.into())
}

pub(super) fn granted_scopes(service: MemberGoogleService, granted: &[String]) -> bool {
    let expected = scopes(service);
    granted.len() == expected.len()
        && expected.iter().all(|scope| {
            granted.iter().any(|value| match value.as_str() {
                "email" => *scope == EMAIL_SCOPE,
                value => value == *scope,
            })
        })
}

pub(super) fn valid_subject(subject: &str) -> bool {
    !subject.is_empty()
        && subject.len() <= 255
        && subject
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
}

#[derive(Deserialize)]
struct UserInfo {
    sub: String,
    email: String,
    email_verified: bool,
}

pub(super) struct ProfileClient {
    client: reqwest::Client,
    #[cfg(test)]
    fixture: Option<url::Url>,
}

impl ProfileClient {
    pub(super) fn google(proxy: SocketAddr) -> Result<Self, &'static str> {
        let invalid = "member_google_proxy_invalid";
        if !proxy.ip().is_loopback() || proxy.port() == 0 {
            return Err(invalid);
        }
        let proxy = reqwest::Proxy::https(format!("http://{proxy}")).map_err(|_| invalid)?;
        Ok(Self {
            client: Self::builder().proxy(proxy).build().map_err(|_| invalid)?,
            #[cfg(test)]
            fixture: None,
        })
    }

    fn builder() -> reqwest::ClientBuilder {
        reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(10))
            .pool_max_idle_per_host(0)
    }

    #[cfg(all(test, feature = "test-fixtures"))]
    pub(super) fn synthetic_loopback(address: SocketAddr) -> Self {
        assert!(address.ip().is_loopback() && address.port() != 0);
        Self {
            client: Self::builder().build().unwrap(),
            fixture: Some(url::Url::parse(&format!("http://{address}/userinfo")).unwrap()),
        }
    }

    pub(super) async fn identity(&self, token: &SecretString) -> Result<Identity, Failure> {
        let target = url::Url::parse(USERINFO).map_err(|_| unavailable())?;
        #[cfg(test)]
        let target = self.fixture.clone().unwrap_or(target);
        let mut response = self
            .client
            .get(target)
            .bearer_auth(token.expose())
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await
            .map_err(|_| unavailable())?;
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(';').next());
        if response.status() != StatusCode::OK
            || !content_type
                .is_some_and(|value| value.trim().eq_ignore_ascii_case("application/json"))
            || response
                .content_length()
                .is_some_and(|len| len > USERINFO_LIMIT as u64)
        {
            return Err(unavailable());
        }
        let mut bytes = Zeroizing::new(Vec::new());
        while let Some(chunk) = response.chunk().await.map_err(|_| unavailable())? {
            if bytes.len().saturating_add(chunk.len()) > USERINFO_LIMIT {
                return Err(unavailable());
            }
            bytes.extend_from_slice(&chunk);
        }
        let profile: UserInfo = serde_json::from_slice(&bytes).map_err(|_| unavailable())?;
        if !profile.email_verified
            || !valid_subject(&profile.sub)
            || profile.email.len() > 320
            || profile
                .email
                .chars()
                .any(|c| c.is_control() || c.is_whitespace())
            || !profile
                .email
                .split_once('@')
                .is_some_and(|(local, domain)| {
                    !local.is_empty() && !domain.is_empty() && !domain.contains('@')
                })
        {
            return Err(unavailable());
        }
        Ok(Identity {
            mailbox: profile.email,
            subject: Some(profile.sub),
        })
    }
}

pub(super) enum WorkspaceRead<'a> {
    Drive,
    Docs(&'a str),
    Slides(&'a str),
    Sheets(&'a str),
}

pub(in crate::personal_google) fn resource_is_valid(
    service: MemberGoogleService,
    resource_id: Option<&str>,
) -> bool {
    match service {
        MemberGoogleService::Gmail => false,
        MemberGoogleService::Drive => resource_id.is_none(),
        _ => resource_id.is_some_and(valid_id),
    }
}

impl<'a> WorkspaceRead<'a> {
    pub(super) fn new(
        service: MemberGoogleService,
        resource_id: Option<&'a str>,
    ) -> Result<Self, Failure> {
        match (service, resource_id) {
            (MemberGoogleService::Drive, None) => Ok(Self::Drive),
            (MemberGoogleService::Docs, Some(id)) if valid_id(id) => Ok(Self::Docs(id)),
            (MemberGoogleService::Slides, Some(id)) if valid_id(id) => Ok(Self::Slides(id)),
            (MemberGoogleService::Sheets, Some(id)) if valid_id(id) => Ok(Self::Sheets(id)),
            _ => Err(Failure(StatusCode::BAD_REQUEST, "invalid_input")),
        }
    }

    pub(super) fn command(&self) -> (&'static [&'static str], Value) {
        match self {
            Self::Drive => (
                &["drive", "files", "list"],
                json!({
                    "q": "trashed = false",
                    "orderBy": "modifiedTime desc",
                    "pageSize": PAGE_SIZE,
                    "fields": "nextPageToken,files(id,name,mimeType)",
                }),
            ),
            Self::Docs(id) => (
                &["docs", "documents", "get"],
                json!({"documentId": id, "fields": "documentId,title"}),
            ),
            Self::Slides(id) => (
                &["slides", "presentations", "get"],
                json!({"presentationId": id, "fields": "presentationId,title,slides(objectId)"}),
            ),
            Self::Sheets(id) => (
                &["sheets", "spreadsheets", "get"],
                json!({"spreadsheetId": id, "fields": "spreadsheetId,properties(title),sheets(properties(title,index))"}),
            ),
        }
    }

    pub(super) fn project(&self, bytes: &[u8]) -> Result<WorkspaceData, Failure> {
        fn read<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, Failure> {
            serde_json::from_slice(bytes).map_err(|_| unavailable())
        }

        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct DrivePage {
            #[serde(default)]
            files: Vec<DriveFile>,
            next_page_token: Option<String>,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct DriveFile {
            id: String,
            name: String,
            mime_type: String,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Doc {
            document_id: String,
            title: String,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Presentation {
            presentation_id: String,
            title: String,
            #[serde(default)]
            slides: Vec<Slide>,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Slide {
            object_id: String,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Spreadsheet {
            spreadsheet_id: String,
            properties: Title,
            #[serde(default)]
            sheets: Vec<Sheet>,
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Title {
            title: String,
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Sheet {
            properties: SheetProperties,
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct SheetProperties {
            title: String,
            #[serde(default)]
            index: u32,
        }

        let answer = match self {
            Self::Drive => {
                let page: DrivePage = read(bytes)?;
                WorkspaceData::Drive(DriveMetadata {
                    files: page
                        .files
                        .into_iter()
                        .map(|file| FileMetadata {
                            id: file.id,
                            name: file.name,
                            mime_type: file.mime_type,
                        })
                        .collect(),
                    has_more: page.next_page_token.is_some_and(|token| !token.is_empty()),
                })
            }
            Self::Docs(id) => {
                let doc: Doc = read(bytes)?;
                if doc.document_id != *id {
                    return Err(unavailable());
                }
                WorkspaceData::Docs(DocMetadata {
                    id: doc.document_id,
                    title: doc.title,
                })
            }
            Self::Slides(id) => {
                let presentation: Presentation = read(bytes)?;
                if presentation.presentation_id != *id
                    || presentation
                        .slides
                        .iter()
                        .any(|slide| !valid_id(&slide.object_id))
                {
                    return Err(unavailable());
                }
                WorkspaceData::Slides(SlidesMetadata {
                    id: presentation.presentation_id,
                    title: presentation.title,
                    slide_count: presentation.slides.len(),
                })
            }
            Self::Sheets(id) => {
                let spreadsheet: Spreadsheet = read(bytes)?;
                if spreadsheet.spreadsheet_id != *id {
                    return Err(unavailable());
                }
                WorkspaceData::Sheets(SheetsMetadata {
                    id: spreadsheet.spreadsheet_id,
                    title: spreadsheet.properties.title,
                    sheets: spreadsheet
                        .sheets
                        .into_iter()
                        .map(|sheet| SheetMetadata {
                            title: sheet.properties.title,
                            index: sheet.properties.index,
                        })
                        .collect(),
                })
            }
        };
        answer.valid().then_some(answer).ok_or_else(unavailable)
    }
}

#[derive(Serialize)]
#[serde(untagged)]
pub(in crate::personal_google) enum WorkspaceData {
    Drive(DriveMetadata),
    Docs(DocMetadata),
    Slides(SlidesMetadata),
    Sheets(SheetsMetadata),
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(in crate::personal_google) struct DriveMetadata {
    files: Vec<FileMetadata>,
    has_more: bool,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct FileMetadata {
    id: String,
    name: String,
    mime_type: String,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(in crate::personal_google) struct DocMetadata {
    id: String,
    title: String,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(in crate::personal_google) struct SlidesMetadata {
    id: String,
    title: String,
    slide_count: usize,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(in crate::personal_google) struct SheetsMetadata {
    id: String,
    title: String,
    sheets: Vec<SheetMetadata>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SheetMetadata {
    title: String,
    index: u32,
}

impl WorkspaceData {
    pub(in crate::personal_google) fn parse(
        service: MemberGoogleService,
        bytes: &[u8],
    ) -> Result<Self, Failure> {
        let invalid = || Failure(StatusCode::BAD_GATEWAY, "google_unavailable");
        if bytes.len() > OUTPUT_LIMIT {
            return Err(invalid());
        }
        let answer = match service {
            MemberGoogleService::Gmail => return Err(invalid()),
            MemberGoogleService::Drive => serde_json::from_slice(bytes).map(Self::Drive),
            MemberGoogleService::Docs => serde_json::from_slice(bytes).map(Self::Docs),
            MemberGoogleService::Slides => serde_json::from_slice(bytes).map(Self::Slides),
            MemberGoogleService::Sheets => serde_json::from_slice(bytes).map(Self::Sheets),
        }
        .map_err(|_| invalid())?;
        answer.valid().then_some(answer).ok_or_else(invalid)
    }

    pub(in crate::personal_google) fn resource_id(&self) -> Option<&str> {
        match self {
            Self::Drive(_) => None,
            Self::Docs(value) => Some(&value.id),
            Self::Slides(value) => Some(&value.id),
            Self::Sheets(value) => Some(&value.id),
        }
    }

    fn valid(&self) -> bool {
        fn text(value: &str) -> bool {
            value.len() <= 1024 && !value.chars().any(char::is_control)
        }
        self.resource_id().is_none_or(valid_id)
            && match self {
                Self::Drive(page) => {
                    let mut ids = std::collections::BTreeSet::new();
                    page.files.len() <= PAGE_SIZE
                        && page.files.iter().all(|file| {
                            valid_id(&file.id)
                                && ids.insert(file.id.as_str())
                                && text(&file.name)
                                && text(&file.mime_type)
                                && !file.mime_type.is_empty()
                        })
                }
                Self::Docs(doc) => text(&doc.title),
                Self::Slides(slides) => text(&slides.title) && slides.slide_count <= SLIDE_LIMIT,
                Self::Sheets(sheets) => {
                    let mut indices = std::collections::BTreeSet::new();
                    text(&sheets.title)
                        && sheets.sheets.len() <= SHEET_LIMIT
                        && sheets.sheets.iter().all(|sheet| {
                            text(&sheet.title)
                                && indices.insert(sheet.index)
                                && sheet.index < SHEET_LIMIT as u32
                        })
                }
            }
    }
}
