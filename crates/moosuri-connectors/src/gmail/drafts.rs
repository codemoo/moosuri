//! Real Gmail drafts CRUD. There is intentionally no drafts/send method.
use super::*;
use mail_parser::MimeHeaders;
#[derive(Deserialize)]
pub struct DraftRef {
    pub id: String,
    pub message: MessageRef,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftPage {
    #[serde(default)]
    pub drafts: Vec<DraftRef>,
    pub next_page_token: Option<String>,
}
pub struct Draft {
    pub id: String,
    pub digest: String,
    pub mail: ParsedMail,
    pub editable: bool,
}
impl GmailClient {
    pub async fn list_drafts(
        &self,
        token: &SecretString,
        page: Option<&str>,
        cancel: &CancellationToken,
    ) -> Result<DraftPage, GmailError> {
        let query = {
            let mut q = url::form_urlencoded::Serializer::new(String::new());
            q.append_pair("maxResults", "20");
            if let Some(page) = page {
                validate_atom(page, 2048)?;
                q.append_pair("pageToken", page);
            }
            q.finish()
        };
        let bytes = self.get(&format!("drafts?{query}"), token, cancel).await?;
        let result: DraftPage = decode(&bytes)?;
        if result.drafts.len() > 20 {
            return Err(GmailError::InvalidResponse);
        }
        for d in &result.drafts {
            validate_id(&d.id)?;
            validate_id(&d.message.id)?;
            validate_id(&d.message.thread_id)?;
        }
        if let Some(p) = &result.next_page_token {
            validate_atom(p, 2048)?;
        }
        Ok(result)
    }
    pub async fn get_draft(
        &self,
        token: &SecretString,
        id: &str,
        cancel: &CancellationToken,
    ) -> Result<Draft, GmailError> {
        validate_id(id)?;
        #[derive(Deserialize)]
        struct Wire {
            id: String,
            message: Raw,
        }
        #[derive(Deserialize)]
        struct Raw {
            raw: String,
        }
        let bytes = self
            .get(&format!("drafts/{id}?format=raw"), token, cancel)
            .await?;
        let result: Wire = decode(&bytes)?;
        if result.id != id || result.message.raw.len() > mime::MAX_MIME_BYTES * 4 / 3 + 8 {
            return Err(GmailError::InvalidResponse);
        }
        let raw = Zeroizing::new(
            URL_SAFE_NO_PAD
                .decode(result.message.raw.trim_end_matches('='))
                .map_err(|_| GmailError::InvalidResponse)?,
        );
        let mail = parse_mail(&raw)?;
        let parsed = mail_parser::MessageParser::default()
            .parse(&*raw)
            .ok_or(GmailError::InvalidResponse)?;
        // Editing a projected HTML, multipart, named-address or reply draft could
        // discard information. Such drafts remain readable/deletable only.
        let unique_headers = ["From", "To", "Cc", "Bcc", "Subject"].iter().all(|name| {
            parsed
                .headers_raw()
                .filter(|(n, _)| n.eq_ignore_ascii_case(name))
                .count()
                <= 1
        });
        let editable = unique_headers
            && parsed.subject().unwrap_or_default().len() <= 2048
            && parsed.parts.len() == 1
            && parsed.text_body_count() == 1
            && matches!(parsed.root_part().body, mail_parser::PartType::Text(_))
            && parsed.root_part().content_type().is_some_and(|ct| {
                ct.ctype().eq_ignore_ascii_case("text")
                    && ct
                        .subtype()
                        .is_some_and(|s| s.eq_ignore_ascii_case("plain"))
            })
            && parsed.attachment_count() == 0
            && !parsed.parts.iter().any(|p| p.is_encoding_problem)
            && parsed.header("In-Reply-To").is_none()
            && parsed.header("References").is_none()
            && [parsed.from(), parsed.to(), parsed.cc(), parsed.bcc()]
                .into_iter()
                .flatten()
                .flat_map(mail_parser::Address::iter)
                .all(|a| a.name.is_none())
            && !mail.text_truncated
            && mail.from.len() == 1;
        Ok(Draft {
            id: id.into(),
            digest: Sha256::digest(&*raw)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect(),
            mail,
            editable,
        })
    }
    pub async fn save_draft(
        &self,
        token: &SecretString,
        id: Option<&str>,
        mail: &FrozenMail,
        cancel: &CancellationToken,
    ) -> Result<DraftRef, GmailError> {
        let (method, path) = if let Some(id) = id {
            validate_id(id)?;
            (reqwest::Method::PUT, format!("drafts/{id}"))
        } else {
            (reqwest::Method::POST, "drafts".into())
        };
        let mut message = serde_json::json!({"raw":URL_SAFE_NO_PAD.encode(mail.raw_mime())});
        if let Some(thread) = mail.thread_id() {
            message["threadId"] = thread.into();
        }
        let bytes = self
            .transport
            .request(
                Endpoint::Gmail,
                method,
                &path,
                Some(token),
                Body::Json(serde_json::json!({"message":message})),
                cancel,
            )
            .await?;
        let result: DraftRef = decode(&bytes)?;
        validate_id(&result.id)?;
        validate_id(&result.message.id)?;
        validate_id(&result.message.thread_id)?;
        if id.is_some_and(|id| id != result.id) {
            return Err(GmailError::InvalidResponse);
        }
        Ok(result)
    }
    pub async fn delete_draft(
        &self,
        token: &SecretString,
        id: &str,
        cancel: &CancellationToken,
    ) -> Result<(), GmailError> {
        validate_id(id)?;
        self.transport
            .request(
                Endpoint::Gmail,
                reqwest::Method::DELETE,
                &format!("drafts/{id}"),
                Some(token),
                Body::Empty,
                cancel,
            )
            .await?;
        Ok(())
    }
}
