//! Conservative v1 equality oracle. No projected/limited ParsedMail is used.
use super::*;
use std::collections::BTreeMap;
/// Non-reversible bounded evidence of the frozen supported message. Never raw mail.
pub fn reconciliation_fingerprint(
    raw: &[u8],
    expected_message_id: &str,
) -> Result<String, GmailError> {
    if raw.is_empty() || raw.len() > mime::MAX_MIME_BYTES {
        return Err(GmailError::LimitExceeded);
    }
    mime::validate_address(expected_message_id)?;
    let raw = std::str::from_utf8(raw).map_err(|_| GmailError::InvalidResponse)?;
    let normalized = raw.replace("\r\n", "\n");
    if normalized.contains('\r') {
        return Err(GmailError::InvalidResponse);
    }
    let (head, body) = normalized
        .split_once("\n\n")
        .ok_or(GmailError::InvalidResponse)?;
    let mut headers = BTreeMap::<String, String>::new();
    let mut last = None::<String>;
    for line in head.lines() {
        if line.starts_with([' ', '\t']) {
            let key = last.as_ref().ok_or(GmailError::InvalidResponse)?;
            let value = headers.get_mut(key).ok_or(GmailError::InvalidResponse)?;
            value.push(' ');
            value.push_str(line.trim());
            continue;
        }
        let (name, value) = line.split_once(':').ok_or(GmailError::InvalidResponse)?;
        let name = name.to_ascii_lowercase();
        if !matches!(
            name.as_str(),
            "from"
                | "to"
                | "cc"
                | "bcc"
                | "subject"
                | "message-id"
                | "in-reply-to"
                | "references"
                | "date"
                | "mime-version"
                | "content-type"
                | "content-transfer-encoding"
        ) || headers.contains_key(&name)
        {
            return Err(GmailError::InvalidResponse);
        }
        headers.insert(name.clone(), value.trim().into());
        last = Some(name);
    }
    if headers.get("message-id").map(String::as_str)
        != Some(format!("<{expected_message_id}>").as_str())
        || !headers.contains_key("from")
        || !headers.contains_key("subject")
    {
        return Err(GmailError::InvalidResponse);
    }
    let ct = headers
        .get("content-type")
        .ok_or(GmailError::InvalidResponse)?
        .to_ascii_lowercase();
    if !ct.starts_with("text/plain") || ct.contains("boundary") {
        return Err(GmailError::InvalidResponse);
    }
    let parsed = mail_parser::MessageParser::default()
        .parse(normalized.as_bytes())
        .ok_or(GmailError::InvalidResponse)?;
    if parsed.parts.len() != 1
        || parsed.parts.iter().any(|p| p.is_encoding_problem)
        || parsed.text_body_count() != 1
        || parsed.attachment_count() != 0
        || parsed.body_text(0).is_none()
    {
        return Err(GmailError::InvalidResponse);
    }
    // Date is generated at freeze time, not an envelope/content identity field.
    // All other supported headers (including Bcc and reply metadata), their
    // complete values/encoding, and the entire body are bound. Only CRLF and
    // header folding are normalized. Provider transformations may stay unknown.
    headers.remove("date");
    let canonical =
        serde_json::to_vec(&(1, headers, body)).map_err(|_| GmailError::InvalidResponse)?;
    let hash = Sha256::digest(canonical);
    Ok(hash.iter().map(|b| format!("{b:02x}")).collect())
}
/// The only successful observation requires a complete, unique Sent result.
/// Fields are bounded metadata, never fetched message plaintext.
#[derive(Clone)]
pub enum SentObservation {
    Matching {
        message_id: String,
        thread_id: String,
    },
    Unresolved {
        status: &'static str,
        candidates: u32,
    },
}
/// One list result cannot authorize a fetch; callers revalidate read authority
/// between these bounded protocol phases.
pub struct SentCandidate {
    rfc_id: String,
    reference: MessageRef,
}
pub enum SentSearch {
    Candidate(SentCandidate),
    Observed(SentObservation),
}
impl GmailClient {
    pub async fn search_sent(
        &self,
        token: &SecretString,
        rfc_id: &str,
        cancel: &CancellationToken,
    ) -> Result<SentSearch, GmailError> {
        mime::validate_address(rfc_id)?;
        // Query is built only from the immutable originally dispatched RFC ID.
        let page = self
            .list_messages(
                token,
                &format!("in:sent rfc822msgid:<{rfc_id}>"),
                None,
                5,
                cancel,
            )
            .await?;
        let count = page.messages.len() as u32;
        let unresolved = |status| {
            Ok(SentSearch::Observed(SentObservation::Unresolved {
                status,
                candidates: count,
            }))
        };
        if page.next_page_token.is_some() {
            return unresolved("incomplete_search");
        }
        if count == 0 {
            return unresolved("no_candidates");
        }
        if count != 1 {
            return unresolved("multiple_candidates");
        }
        Ok(SentSearch::Candidate(SentCandidate {
            rfc_id: rfc_id.into(),
            reference: page.messages.into_iter().next().unwrap(),
        }))
    }
    pub async fn inspect_sent(
        &self,
        token: &SecretString,
        candidate: SentCandidate,
        fingerprint: Option<&str>,
        reply_thread: Option<&str>,
        cancel: &CancellationToken,
    ) -> Result<SentObservation, GmailError> {
        let rfc_id = &candidate.rfc_id;
        let candidate = &candidate.reference;
        let unresolved = |status| {
            Ok(SentObservation::Unresolved {
                status,
                candidates: 1,
            })
        };

        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Wire {
            id: String,
            thread_id: String,
            #[serde(default)]
            label_ids: Vec<String>,
            raw: String,
        }
        let bytes = self
            .get(
                &format!(
                    "messages/{}?format=raw&fields=id,threadId,labelIds,raw",
                    candidate.id
                ),
                token,
                cancel,
            )
            .await?;
        let w: Wire = decode(&bytes)?;
        if w.id != candidate.id
            || w.thread_id != candidate.thread_id
            || w.label_ids.len() > 128
            || !w.label_ids.iter().any(|l| l == "SENT")
            || reply_thread.is_some_and(|t| t != w.thread_id)
        {
            return unresolved("candidate_mismatch");
        }
        if w.raw.len() > mime::MAX_MIME_BYTES * 4 / 3 + 8 {
            return unresolved("unsupported_candidate");
        }
        let raw = zeroize::Zeroizing::new(
            URL_SAFE_NO_PAD
                .decode(w.raw.trim_end_matches('='))
                .map_err(|_| GmailError::InvalidResponse)?,
        );
        let actual = match reconciliation_fingerprint(&raw, rfc_id) {
            Ok(f) => f,
            Err(_) => return unresolved("unsupported_candidate"),
        };
        match fingerprint {
            Some(expected) if expected == actual => Ok(SentObservation::Matching {
                message_id: w.id,
                thread_id: w.thread_id,
            }),
            Some(_) => unresolved("candidate_mismatch"),
            None => unresolved("legacy_evidence_unavailable"),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn complete_fingerprint_rejects_hidden_or_missing_content() {
        let c = MailContent {
            from: "a@example.test".into(),
            to: vec!["b@example.test".into()],
            cc: vec![],
            bcc: vec!["c@example.test".into()],
            subject: "Synthetic".into(),
            text: "Complete body".into(),
            reply: None,
        };
        let f = FrozenMail::prepare(&c, "id@example.test").unwrap();
        let raw = std::str::from_utf8(f.raw_mime()).unwrap();
        let h = reconciliation_fingerprint(f.raw_mime(), "id@example.test").unwrap();
        assert_eq!(
            h,
            reconciliation_fingerprint(raw.replace("\r\n", "\n").as_bytes(), "id@example.test")
                .unwrap()
        );
        assert!(
            reconciliation_fingerprint(
                format!("X-Hidden: no\r\n{raw}").as_bytes(),
                "id@example.test"
            )
            .is_err()
        );
        assert!(
            reconciliation_fingerprint(
                format!("Subject: duplicate\r\n{raw}").as_bytes(),
                "id@example.test"
            )
            .is_err()
        );
        assert!(reconciliation_fingerprint(f.raw_mime(), "wrong@example.test").is_err());
        assert_ne!(
            h,
            reconciliation_fingerprint(
                raw.replace("Complete body", "Other body").as_bytes(),
                "id@example.test"
            )
            .unwrap()
        );
        let missing = raw
            .lines()
            .filter(|l| !l.starts_with("Bcc:"))
            .collect::<Vec<_>>()
            .join("\n");
        assert_ne!(
            h,
            reconciliation_fingerprint(missing.as_bytes(), "id@example.test").unwrap()
        );
    }
}
