//! Header-only current-INBOX projection. No MIME bodies or snippets.
use super::*;
pub struct InboxMetadata {
    pub id: String,
    pub received_at_ms: i64,
    pub subject: String,
    pub from: String,
    pub date: String,
}
impl GmailClient {
    pub async fn inbox_metadata(
        &self,
        token: &SecretString,
        id: &str,
        cancel: &CancellationToken,
    ) -> Result<InboxMetadata, GmailError> {
        validate_id(id)?;
        let bytes = self.get(&format!("messages/{id}?format=metadata&metadataHeaders=Subject&metadataHeaders=From&metadataHeaders=Date&fields=id,labelIds,internalDate,payload(headers)"), token, cancel).await?;
        InboxMetadata::from_gmail_json(id, &bytes)
    }
}
impl InboxMetadata {
    pub fn from_gmail_json(id: &str, bytes: &[u8]) -> Result<Self, GmailError> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Wire {
            id: String,
            label_ids: Vec<String>,
            internal_date: String,
            payload: Payload,
        }
        #[derive(Deserialize)]
        struct Payload {
            #[serde(default)]
            headers: Vec<Header>,
        }
        #[derive(Deserialize)]
        struct Header {
            name: String,
            value: String,
        }
        let wire: Wire = decode(bytes)?;
        let received_at_ms = wire
            .internal_date
            .parse::<i64>()
            .map_err(|_| GmailError::InvalidResponse)?;
        if wire.id != id
            || validate_id(id).is_err()
            || received_at_ms < 0
            || wire.label_ids.len() > 256
            || !wire.label_ids.iter().any(|v| v == "INBOX")
            || wire.payload.headers.len() > 32
        {
            return Err(GmailError::InvalidResponse);
        }
        let header = |name: &str| -> Result<String, GmailError> {
            let values = wire
                .payload
                .headers
                .iter()
                .filter(|h| h.name.eq_ignore_ascii_case(name))
                .collect::<Vec<_>>();
            if values.len() > 1 {
                return Err(GmailError::InvalidResponse);
            }
            let value = values.first().map(|h| h.value.as_str()).unwrap_or("");
            // Flatten untrusted headers into bounded literal one-line text.
            Ok(value
                .chars()
                .map(|c| if c.is_control() { ' ' } else { c })
                .take(512)
                .collect())
        };
        Ok(Self {
            subject: header("subject")?,
            from: header("from")?,
            date: header("date")?,
            id: wire.id,
            received_at_ms,
        })
    }
}
