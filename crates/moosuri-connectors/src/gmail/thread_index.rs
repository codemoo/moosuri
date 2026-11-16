//! Header-only polling; neither bodies nor models are touched on an unchanged thread.
use super::*;
use std::collections::HashSet;

pub struct MailThreadIndex {
    pub id: String,
    pub messages: Vec<MailMessageIndex>,
}
pub struct MailMessageIndex {
    pub id: String,
    pub received_at_ms: i64,
    pub outbound: bool,
}
impl MailThreadIndex {
    /// Start at the actual observed message, not the time of the first poll.
    /// This includes a reply arriving between the read/send and registration.
    pub fn new_inbound(
        &self,
        baseline: &[String],
        seen: &[String],
    ) -> Result<Vec<String>, GmailError> {
        if baseline.is_empty() {
            return Err(GmailError::InvalidResponse);
        }
        let cutoff = baseline
            .iter()
            .map(|id| {
                self.messages
                    .iter()
                    .find(|m| &m.id == id)
                    .map(|m| m.received_at_ms)
                    .ok_or(GmailError::InvalidResponse)
            })
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .max()
            .ok_or(GmailError::InvalidResponse)?;
        let mut messages = self
            .messages
            .iter()
            .filter(|m| !m.outbound && m.received_at_ms >= cutoff && !seen.contains(&m.id))
            .collect::<Vec<_>>();
        messages.sort_by_key(|m| (m.received_at_ms, &m.id));
        Ok(messages.into_iter().map(|m| m.id.clone()).collect())
    }
}
impl GmailClient {
    pub async fn thread_index(
        &self,
        token: &SecretString,
        id: &str,
        cancel: &CancellationToken,
    ) -> Result<MailThreadIndex, GmailError> {
        validate_id(id)?;
        #[derive(Deserialize)]
        struct Wire {
            id: String,
            messages: Vec<Item>,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Item {
            id: String,
            thread_id: String,
            label_ids: Vec<String>,
            internal_date: String,
        }
        let bytes = self.get(&format!("threads/{id}?format=metadata&metadataHeaders=Message-ID&fields=id,messages(id,threadId,labelIds,internalDate)"), token, cancel).await?;
        let wire: Wire = decode(&bytes)?;
        if wire.id != id || wire.messages.is_empty() || wire.messages.len() > 512 {
            return Err(GmailError::InvalidResponse);
        }
        let mut ids = HashSet::new();
        let mut messages = Vec::new();
        for item in wire.messages {
            validate_id(&item.id)?;
            let date = item
                .internal_date
                .parse::<i64>()
                .map_err(|_| GmailError::InvalidResponse)?;
            if item.thread_id != id
                || date < 0
                || item.label_ids.len() > 256
                || !ids.insert(item.id.clone())
            {
                return Err(GmailError::InvalidResponse);
            }
            messages.push(MailMessageIndex {
                id: item.id,
                received_at_ms: date,
                outbound: item.label_ids.iter().any(|v| v == "SENT" || v == "DRAFT"),
            });
        }
        Ok(MailThreadIndex {
            id: wire.id,
            messages,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reply_watch_uses_latest_observed_delivery_not_vector_order_or_first_poll_time() {
        let index = MailThreadIndex {
            id: "thread".into(),
            messages: vec![
                MailMessageIndex {
                    id: "latest-sent".into(),
                    received_at_ms: 300,
                    outbound: true,
                },
                MailMessageIndex {
                    id: "old-reply".into(),
                    received_at_ms: 200,
                    outbound: false,
                },
                MailMessageIndex {
                    id: "read".into(),
                    received_at_ms: 100,
                    outbound: false,
                },
                MailMessageIndex {
                    id: "reply-before-registration".into(),
                    received_at_ms: 400,
                    outbound: false,
                },
                MailMessageIndex {
                    id: "same-time-new-reply".into(),
                    received_at_ms: 300,
                    outbound: false,
                },
                MailMessageIndex {
                    id: "draft".into(),
                    received_at_ms: 500,
                    outbound: true,
                },
            ],
        };
        let baseline = vec!["read".into(), "latest-sent".into()];
        assert_eq!(
            index.new_inbound(&baseline, &baseline).unwrap(),
            vec!["same-time-new-reply", "reply-before-registration"]
        );
        assert!(
            index
                .new_inbound(&["removed-message".into()], &baseline)
                .is_err()
        );
        assert!(
            index
                .new_inbound(
                    &baseline,
                    &index
                        .messages
                        .iter()
                        .map(|m| m.id.clone())
                        .collect::<Vec<_>>()
                )
                .unwrap()
                .is_empty()
        );
    }
}
