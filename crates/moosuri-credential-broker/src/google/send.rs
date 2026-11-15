//! Private token preflight and one exact approved effect. No network resend.
use super::*;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use moosuri_connectors::gmail::{
    FrozenMail, SecretString, SentObservation, SentSearch, validate_native_send,
};
use moosuri_connectors::model::CancellationToken;
impl crate::Engine {
    pub(crate) fn attach_mail_payload(
        &mut self,
        invocation: &Invocation,
        payload: GoogleMailPayload,
    ) -> Result<()> {
        let request = &self
            .state
            .entries
            .iter()
            .find(|e| &e.request.invocation == invocation)
            .ok_or(Error::Policy)?
            .request;
        let execution = match request.operation {
            Operation::GoogleMailSend { execution, .. }
            | Operation::GoogleMailLookup { execution, .. } => execution,
            _ => return Err(Error::Policy),
        };
        if payload.owner != request.device.owner
            || auth::hash(&payload.execution_bytes()) != execution
            || (matches!(request.operation, Operation::GoogleMailSend { .. })
                && payload.expires_at < request.expires_at)
            || payload.revision == 0
            || payload.action.is_nil()
            || payload.connection.is_nil()
            || payload.message_id.len() > 254
            || payload.fingerprint.len() != 64
            || payload.raw.as_ref().is_some_and(|s| s.len() > 699_056)
        {
            return Err(Error::Policy);
        }
        if matches!(request.operation, Operation::GoogleMailSend { .. }) != payload.raw.is_some() {
            return Err(Error::Policy);
        }
        self.mail_payloads.retain(|(i, _)| i != invocation);
        self.mail_payloads.push((invocation.clone(), payload));
        Ok(())
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn execute_mail_secret(
        &mut self,
        invocation: &Invocation,
        adapter: &GoogleAdapter,
        secret: &str,
        item_epoch: u64,
        key_epoch: u64,
        mut vault: Option<&mut crate::live_vault::LiveVault>,
    ) -> Result<SignedReceipt> {
        let request = self
            .state
            .entries
            .iter()
            .find(|e| &e.request.invocation == invocation)
            .ok_or(Error::Policy)?
            .request
            .clone();
        let grant = self
            .grants()
            .iter()
            .find(|g| g.binding.id == request.binding.id)
            .ok_or(Error::Policy)?
            .clone();
        let payload = self
            .mail_payloads
            .iter()
            .find(|(i, _)| i == invocation)
            .map(|(_, p)| p.clone())
            .ok_or(Error::Policy)?;
        if matches!(request.operation, Operation::GoogleMailSend { .. })
            && (payload.not_before > current_time()? || payload.expires_at <= current_time()?)
        {
            return self.settle_without_retry(invocation);
        }
        let identity = grant.google().ok_or(Error::Policy)?;
        let (credential, token) = match adapter.authorize(identity, secret, true).await {
            Ok(value) => value,
            Err(error) => {
                if error == Error::Policy {
                    self.state
                        .grants
                        .iter_mut()
                        .find(|g| g.binding.id == grant.binding.id)
                        .ok_or(Error::Policy)?
                        .suspend_google();
                    self.save()?;
                }
                return self.settle_without_retry(invocation);
            }
        };
        if let Some(vault) = vault.as_mut() {
            vault.check(&grant).await?;
        }
        let contains_secret = |value: &str| {
            [
                Some(secret),
                Some(credential.client_id.expose()),
                credential.client_secret.as_ref().map(|s| s.expose()),
                Some(credential.refresh_token.expose()),
                Some(token.access_token.expose()),
            ]
            .into_iter()
            .flatten()
            .any(|s| !s.is_empty() && value.contains(s))
        };
        let frozen = if let Some(raw) = &payload.raw {
            let bytes = Zeroizing::new(URL_SAFE_NO_PAD.decode(raw).map_err(|_| Error::Policy)?);
            if std::str::from_utf8(&bytes)
                .map_err(|_| Error::Policy)
                .is_ok_and(contains_secret)
            {
                return self.settle_without_retry(invocation);
            }
            let mail = validate_native_send(&bytes, &payload.message_id, &identity.email)
                .map_err(|_| Error::Policy)?;
            if [&mail.subject, &mail.text]
                .into_iter()
                .chain(mail.from.iter())
                .chain(mail.to.iter())
                .chain(mail.cc.iter())
                .chain(mail.bcc.iter())
                .any(|s| contains_secret(s))
            {
                return self.settle_without_retry(invocation);
            }
            if moosuri_connectors::gmail::reconciliation_fingerprint(&bytes, &payload.message_id)
                .map_err(|_| Error::Policy)?
                != payload.fingerprint
            {
                return Err(Error::Policy);
            }
            Some(
                FrozenMail::restore(bytes.to_vec(), payload.mime_digest, None)
                    .map_err(|_| Error::Policy)?,
            )
        } else {
            None
        };
        let transport = self.mail_transport.clone().ok_or(Error::Unavailable)?;
        if !matches!(
            transport
                .command(
                    self,
                    Command::CheckMail {
                        invocation: invocation.clone(),
                        request_digest: auth::hash(&request_bytes(&request))
                    }
                )
                .await,
            Ok(Reply::MailCurrent)
        ) {
            return self.settle_without_retry(invocation);
        }
        // Durable entry precedes the only send POST. Any failure from here is never replayed.
        let commitment = self.begin_dispatch(invocation, current_time()?, item_epoch, key_epoch)?;
        let token =
            SecretString::new(token.access_token.expose().to_owned()).map_err(|_| Error::Policy)?;
        let cancel = CancellationToken::new();
        let result = tokio::time::timeout(Duration::from_secs(60), async {
            if let Some(frozen) = frozen {
                let receipt = adapter
                    .mail
                    .send_frozen(&token, &frozen, &cancel)
                    .await
                    .map_err(|error| match error {
                        moosuri_connectors::gmail::GmailError::Http(
                            400 | 401 | 403 | 404 | 413,
                        ) => Error::Policy,
                        _ => Error::Unknown,
                    })?;
                if contains_secret(&receipt.id) || contains_secret(&receipt.thread_id) {
                    return Err(Error::ProviderContent);
                }
                Ok(Projection::GoogleMailSent {
                    message_id: receipt.id,
                    thread_id: receipt.thread_id,
                })
            } else {
                let result = match adapter
                    .mail
                    .search_sent(&token, &payload.message_id, &cancel)
                    .await
                    .map_err(|_| Error::Unknown)?
                {
                    SentSearch::Observed(observation) => observation,
                    SentSearch::Candidate(candidate) => {
                        if let Some(vault) = vault.as_mut() {
                            vault.check(&grant).await?;
                        }
                        if !matches!(
                            transport
                                .command(
                                    self,
                                    Command::CheckMail {
                                        invocation: invocation.clone(),
                                        request_digest: auth::hash(&request_bytes(&request))
                                    }
                                )
                                .await,
                            Ok(Reply::MailCurrent)
                        ) {
                            return Err(Error::Policy);
                        }
                        adapter
                            .mail
                            .inspect_sent(
                                &token,
                                candidate,
                                Some(&payload.fingerprint),
                                None,
                                &cancel,
                            )
                            .await
                            .map_err(|_| Error::Unknown)?
                    }
                };
                let (status, candidates, message_id, thread_id) = match result {
                    SentObservation::Matching {
                        message_id,
                        thread_id,
                    } => {
                        if contains_secret(&message_id) || contains_secret(&thread_id) {
                            return Err(Error::ProviderContent);
                        }
                        (
                            GoogleSentStatus::Matching,
                            1,
                            Some(message_id),
                            Some(thread_id),
                        )
                    }
                    SentObservation::Unresolved { status, candidates } => (
                        match status {
                            "no_candidates" => GoogleSentStatus::NoCandidates,
                            "multiple_candidates" => GoogleSentStatus::MultipleCandidates,
                            "incomplete_search" => GoogleSentStatus::IncompleteSearch,
                            "candidate_mismatch" => GoogleSentStatus::CandidateMismatch,
                            "unsupported_candidate" => GoogleSentStatus::UnsupportedCandidate,
                            "legacy_evidence_unavailable" => {
                                GoogleSentStatus::LegacyEvidenceUnavailable
                            }
                            _ => GoogleSentStatus::LookupUnavailable,
                        },
                        candidates.min(5) as u8,
                        None,
                        None,
                    ),
                };
                Ok(Projection::GoogleMailObserved {
                    status,
                    candidates,
                    message_id,
                    thread_id,
                })
            }
        })
        .await
        .unwrap_or(Err(Error::Unknown));
        let (outcome, projection) = match result {
            Ok(p) => (Outcome::ProjectedSuccess, p),
            Err(Error::Policy) => (Outcome::Rejected, Projection::None),
            Err(_) => (Outcome::Unknown, Projection::None),
        };
        let receipt = Receipt {
            commitment,
            outcome,
            projection_digest: auth::projection_digest(projection.clone()),
            projection,
        };
        let signed = SignedReceipt {
            signature: self.key.sign(&receipt_bytes(&receipt)),
            receipt,
        };
        self.state
            .entries
            .iter_mut()
            .find(|e| &e.request.invocation == invocation)
            .ok_or(Error::Policy)?
            .phase = Phase::Settled(signed.clone());
        self.mail_payloads.retain(|(i, _)| i != invocation);
        self.save()?;
        Ok(signed)
    }
}
