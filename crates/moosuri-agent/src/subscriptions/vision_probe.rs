//! Synthetic, bounded image proof using the same official transports as saved turns.
use super::*;
use moosuri_connectors::model::protocol::{
    FinishOutcome, ModelEvent, TextMessage, TextRequest, TextRole,
};
use moosuri_core::{ModelChoice, ModelSelection, ModelSource};
impl SubscriptionService {
    pub async fn personal_vision_probe(
        &self,
        u: &UserContext,
        selection: ModelSelection,
    ) -> Result<ModelChoice, StoreError> {
        let _sync = self
            .claude_model_sync
            .try_lock()
            .map_err(|_| StoreError::Busy)?;
        let proof = self
            .store
            .personal_vision_probe_binding(u, selection)
            .await?;
        let (image, expected) = moosuri_store::media::vision_challenge()?;
        let request=TextRequest{model:proof.binding.model.clone(),effort:proof.binding.effort.clone(),max_output_tokens:256,messages:vec![TextMessage{role:TextRole::User,text:"Read the 4 by 4 colored tile image row by row, left to right. Return exactly 16 uppercase letters, one per tile: R=red, G=green, B=blue, Y=yellow. No spaces, punctuation or explanation.".into(),phase:None}]};
        let cancel = CancellationToken::new();
        let (tx, mut rx) = tokio::sync::mpsc::channel(16);
        let profile;
        let client;
        let images;
        let mut operation: std::pin::Pin<
            Box<
                dyn std::future::Future<
                        Output = Result<(), moosuri_connectors::model::TransportError>,
                    > + Send
                    + '_,
            >,
        >;
        if let Some(reference) = proof.binding.claude_code.as_ref() {
            profile = self.claude_profile().await?;
            if profile.digest() != reference.profile_digest
                || profile.root_id() != reference.root_id
            {
                return Err(StoreError::Conflict);
            }
            images = vec![
                moosuri_connectors::model::NormalizedJpeg::new(image)
                    .map_err(|_| StoreError::InvalidInput)?,
            ];
            operation = Box::pin(profile.stream_text_with_images(
                &reference.environment_id,
                &request,
                &images,
                &tx,
                &cancel,
            ));
        } else {
            let reference = proof
                .binding
                .subscription
                .clone()
                .ok_or(StoreError::Conflict)?;
            client = self
                .client(SubscriptionAuthority::User(u.clone()), reference)
                .await?
                .with_images(vec![image])
                .map_err(|_| StoreError::Unavailable)?;
            operation = Box::pin(client.stream_text(&request, &tx, &cancel));
        }
        self.store.reserve_personal_vision_probe(u, &proof).await?;
        let mut tick = tokio::time::interval(Duration::from_millis(250));
        let mut model = None;
        let mut text = String::new();
        let mut finish = None;
        let mut invalid = false;
        let mut observe = |e| match e {
            ModelEvent::Started { model: m, .. } => model = m,
            ModelEvent::TextDelta { text: t, .. } => {
                if text.len() + t.len() > 128 {
                    invalid = true;
                } else {
                    text.push_str(&t);
                }
            }
            ModelEvent::Finished { outcome, .. } => finish = Some(outcome),
            ModelEvent::RefusalDelta { .. } => invalid = true,
            _ => {}
        };
        let result=tokio::time::timeout(Duration::from_secs(90),async {
            loop {tokio::select!{biased;
                _=self.signals.stopping().cancelled()=>{cancel.cancel();return Err(StoreError::Unavailable);},
                _=tick.tick()=>{if let Err(e)=self.store.validate_personal_vision_probe(u,&proof).await {cancel.cancel();return Err(e);}},
                Some(e)=rx.recv()=>observe(e),
                r=&mut operation=>return r.map_err(|_|StoreError::Unavailable),
            }}
        }).await;
        cancel.cancel();
        result.map_err(|_| StoreError::Unavailable)??;
        while let Ok(e) = rx.try_recv() {
            observe(e);
        }
        if invalid
            || model.as_deref() != Some(&request.model)
            || text.trim() != expected
            || finish != Some(FinishOutcome::Completed)
        {
            return Err(StoreError::Unavailable);
        }
        drop(operation);
        self.store.record_personal_vision_probe(u, proof).await
    }
    pub async fn claude_vision_probe(
        &self,
        u: &UserContext,
        id: String,
        revision: i64,
        model: String,
        effort: Option<String>,
    ) -> Result<ModelChoice, StoreError> {
        self.personal_vision_probe(
            u,
            ModelSelection {
                source: ModelSource::PersonalClaudeCode,
                provider_id: id,
                provider_revision: revision,
                model,
                effort,
            },
        )
        .await
    }
}
