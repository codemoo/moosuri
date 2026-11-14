//! Owner-scoped immutable image bytes. Never Debug/Serialize binary carriers.
use super::*;
use image::{ImageDecoder, ImageFormat, ImageReader, codecs::jpeg::JpegEncoder};
use moosuri_core::{MediaLibrary, MediaSummary};
use std::io::Cursor;
pub const MAX_BYTES: usize = 2 * 1024 * 1024;
pub const QUOTA_BYTES: usize = 64 * 1024 * 1024;
pub struct NormalizedImage {
    pub bytes: Vec<u8>,
    pub thumbnail: Vec<u8>,
    pub width: u32,
    pub height: u32,
}
pub struct MediaUpload {
    pub(crate) user: UserContext,
    pub(crate) id: String,
    pub(crate) nonce: String,
}
type UploadRow = (String, Option<Vec<u8>>, String, i64, Option<Vec<u8>>);
pub enum UploadAdmission {
    Ready(MediaSummary),
    Process(MediaUpload),
}
/// Caller bounds concurrent buffers/decode before invoking this CPU-only function.
pub fn normalize(input: &[u8]) -> Result<NormalizedImage> {
    if input.is_empty() || input.len() > MAX_BYTES {
        return Err(StoreError::InvalidInput);
    }
    let format = image::guess_format(input).map_err(|_| StoreError::InvalidInput)?;
    if !matches!(
        format,
        ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::WebP
    ) {
        return Err(StoreError::InvalidInput);
    }
    let mut reader = ImageReader::with_format(Cursor::new(input), format);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(2400);
    limits.max_image_height = Some(2400);
    limits.max_alloc = Some(48 * 1024 * 1024);
    reader.limits(limits);
    let mut decoder = reader
        .into_decoder()
        .map_err(|_| StoreError::InvalidInput)?;
    let (width, height) = decoder.dimensions();
    if width == 0
        || height == 0
        || width > 2400
        || height > 2400
        || u64::from(width) * u64::from(height) > 5_760_000
    {
        return Err(StoreError::InvalidInput);
    }
    let orientation = decoder
        .orientation()
        .map_err(|_| StoreError::InvalidInput)?;
    let mut decoded =
        image::DynamicImage::from_decoder(decoder).map_err(|_| StoreError::InvalidInput)?;
    decoded.apply_orientation(orientation);
    let rgb = decoded.to_rgb8();
    let mut bytes = Vec::new();
    JpegEncoder::new_with_quality(&mut bytes, 85)
        .encode_image(&rgb)
        .map_err(|_| StoreError::InvalidInput)?;
    if bytes.len() > MAX_BYTES {
        return Err(StoreError::InvalidInput);
    }
    let thumbnail = decoded.thumbnail(256, 256).to_rgb8();
    let mut thumb = Vec::new();
    JpegEncoder::new_with_quality(&mut thumb, 75)
        .encode_image(&thumbnail)
        .map_err(|_| StoreError::InvalidInput)?;
    if thumb.len() > 131072 {
        return Err(StoreError::InvalidInput);
    }
    Ok(NormalizedImage {
        bytes,
        thumbnail: thumb,
        width: rgb.width(),
        height: rgb.height(),
    })
}
fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<MediaSummary> {
    Ok(MediaSummary {
        id: r.get(0)?,
        revision: r.get(1)?,
        digest: r.get(2)?,
        state: r.get(3)?,
        width: r.get(4)?,
        height: r.get(5)?,
        bytes: r.get(6)?,
        created_at: r.get(7)?,
        filename: r.get(8)?,
        media_type: r.get(9)?,
    })
}
const SELECT: &str = "SELECT id,revision,digest,state,width,height,coalesce(length(bytes),0),created_at,filename,media_type FROM media_objects";
pub(crate) fn summary(conn: &Connection, tenant: &str, id: &str) -> Result<MediaSummary> {
    conn.query_row(
        &format!("{SELECT} WHERE tenant_id=?1 AND id=?2"),
        params![tenant, id],
        row,
    )
    .optional()?
    .ok_or(StoreError::NotFound)
}
pub fn quarantine(conn: &Connection) -> Result<()> {
    conn.execute(
        "UPDATE media_objects SET state='retry_required',nonce=NULL WHERE state='processing'",
        [],
    )?;
    Ok(())
}
pub(super) fn turn_media(
    conn: &Connection,
    tenant: &str,
    run: &str,
) -> Result<Option<Vec<MediaSummary>>> {
    let mut stmt=conn.prepare(&format!("{SELECT} WHERE tenant_id=?1 AND id IN(SELECT media_id FROM chat_media WHERE tenant_id=?1 AND run_id=?2) ORDER BY created_at,id"))?;
    let items = stmt
        .query_map(params![tenant, run], row)?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok((!items.is_empty()).then_some(items))
}
pub(super) fn apply_delete(conn: &Connection, tenant: &str, account: &str, id: &str) -> Result<()> {
    // A tombstone also wins when cancellation arrives before upload admission.
    conn.execute("INSERT INTO media_objects(tenant_id,account_id,id,state,auth_epoch,created_at) VALUES(?1,?2,?3,'deleted',0,?4) ON CONFLICT(tenant_id,id) DO UPDATE SET state='deleted',bytes=NULL,thumbnail=NULL,extracted_text=NULL,nonce=NULL",params![tenant,account,id,now()?])?;
    let runs = conn
        .prepare("SELECT run_id FROM media_run_dependencies WHERE tenant_id=?1 AND media_id=?2")?
        .query_map(params![tenant, id], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    // A selected run may still be queued and have no execution row. Seal the
    // revocation before purge emits retry metadata, including that case.
    for run in &runs {
        conn.execute("INSERT INTO agent_execution(tenant_id,run_id,agent,invalidated) VALUES(?1,?2,1,1) ON CONFLICT(tenant_id,run_id) DO UPDATE SET invalidated=1",params![tenant,run])?;
    }
    agent_execution::purge_runs(conn, tenant, runs)?;
    Ok(())
}
/// Same ready, non-withdrawn conversation prefix used by attachment inheritance.
/// P1 personal text must reject this before copying dependencies or loading bytes.
pub(super) fn thread_has_context(
    conn: &Connection,
    tenant: &str,
    account: &str,
    thread: &str,
) -> Result<bool> {
    Ok(conn.query_row("SELECT EXISTS(SELECT 1 FROM media_run_dependencies d JOIN chat_runs r ON r.tenant_id=d.tenant_id AND r.id=d.run_id JOIN media_objects m ON m.tenant_id=d.tenant_id AND m.id=d.media_id WHERE r.tenant_id=?1 AND r.account_id=?2 AND r.thread_id=?3 AND m.state='ready' AND NOT EXISTS(SELECT 1 FROM agent_execution a WHERE a.tenant_id=r.tenant_id AND a.run_id=r.id AND a.invalidated=1))",params![tenant,account,thread],|r|r.get(0))?)
}
pub(super) fn has_context(conn: &Connection, tenant: &str, run: &str) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM media_run_dependencies WHERE tenant_id=?1 AND run_id=?2)",
        params![tenant, run],
        |r| r.get(0),
    )?)
}
pub(super) fn bound_images(conn: &Connection, tenant: &str, run: &str) -> Result<Vec<Vec<u8>>> {
    Ok(conn.prepare("SELECT m.bytes FROM media_objects m JOIN chat_media c ON c.tenant_id=m.tenant_id AND c.media_id=m.id AND c.revision=m.revision WHERE c.tenant_id=?1 AND c.run_id=?2 AND m.state='ready' AND m.filename IS NULL ORDER BY c.ordinal")?.query_map(params![tenant,run],|r|r.get(0))?.collect::<std::result::Result<Vec<_>,_>>()?)
}
impl Store {
    pub async fn media_begin(
        &self,
        user: &UserContext,
        id: String,
        digest: [u8; 32],
    ) -> Result<UploadAdmission> {
        self.attachment_begin(user, id, digest, None).await
    }
    pub(crate) async fn attachment_begin(
        &self,
        user: &UserContext,
        id: String,
        digest: [u8; 32],
        file: Option<(String, String)>,
    ) -> Result<UploadAdmission> {
        let user = user.clone();
        self.call(move|db|{
            let user=validate(&db.conn,&user,true,false)?;
            let id=Uuid::parse_str(&id).map_err(|_|StoreError::InvalidInput)?.to_string();
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let old:Option<UploadRow>=tx.query_row("SELECT state,input_digest,account_id,auth_epoch,session_digest FROM media_objects WHERE tenant_id=?1 AND id=?2",params![user.tenant.0,id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional()?;
            let existing=old.is_some();
            if existing {
                let name:Option<String>=tx.query_row("SELECT filename FROM media_objects WHERE tenant_id=?1 AND id=?2",params![user.tenant.0,id],|r|r.get(0))?;
                if name.as_deref()!=file.as_ref().map(|v|v.0.as_str()) {return Err(StoreError::Conflict)}
            }
            if let Some((state,old,owner,epoch,session))=old {
                if owner!=user.account.id || state=="deleted" || old.as_deref()!=Some(digest.as_slice()){return Err(StoreError::Conflict)}
                if state=="ready" { return Ok(UploadAdmission::Ready(summary(&tx,&user.tenant.0,&id)?)); }
                if state=="processing" && epoch==user.epoch && session.as_deref()==Some(user.digest.as_slice()) { return Err(StoreError::Busy); }
            }
            let used:i64=tx.query_row("SELECT coalesce(sum(CASE WHEN state='processing' THEN 2228224 ELSE coalesce(length(bytes),0)+coalesce(length(thumbnail),0)+coalesce(length(CAST(extracted_text AS BLOB)),0) END),0) FROM media_objects WHERE account_id=?1 AND id!=?2 AND state!='deleted'",params![user.account.id,id],|r|r.get(0))?;
            let count:i64=tx.query_row("SELECT count(*) FROM media_objects WHERE account_id=?1",[&user.account.id],|r|r.get(0))?;
            if used+MAX_BYTES as i64+131072>QUOTA_BYTES as i64 || (!existing && count>=4096) { return Err(StoreError::Busy); }
            let nonce=Uuid::new_v4().to_string();
            tx.execute("INSERT INTO media_objects(tenant_id,account_id,id,input_digest,state,nonce,auth_epoch,created_at,session_digest,filename,media_type) VALUES(?1,?2,?3,?4,'processing',?5,?6,?7,?8,?9,?10) ON CONFLICT(tenant_id,id) DO UPDATE SET state='processing',nonce=?5,auth_epoch=?6,session_digest=?8",params![user.tenant.0,user.account.id,id,digest.as_slice(),nonce,user.epoch,now()?,user.digest.as_slice(),file.as_ref().map(|v|&v.0),file.as_ref().map(|v|&v.1)])?;
            tx.commit()?; Ok(UploadAdmission::Process(MediaUpload{user,id,nonce}))
        }).await
    }
    pub async fn media_finish(
        &self,
        upload: MediaUpload,
        image: Option<NormalizedImage>,
    ) -> Result<MediaSummary> {
        self.call(move|db|{
            let user=validate(&db.conn,&upload.user,true,false)?;
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let n=if let Some(image)=image {
                let digest=format!("{:x}",Sha256::digest(&image.bytes));
                tx.execute("UPDATE media_objects SET state='ready',nonce=NULL,digest=?5,width=?6,height=?7,bytes=?8,thumbnail=?9 WHERE tenant_id=?1 AND id=?2 AND nonce=?3 AND auth_epoch=?4 AND state='processing' AND filename IS NULL",params![user.tenant.0,upload.id,upload.nonce,user.epoch,digest,image.width,image.height,image.bytes,image.thumbnail])?
            }else{tx.execute("UPDATE media_objects SET state='retry_required',nonce=NULL WHERE tenant_id=?1 AND id=?2 AND nonce=?3 AND auth_epoch=?4 AND state='processing'",params![user.tenant.0,upload.id,upload.nonce,user.epoch])?};
            if n!=1{return Err(StoreError::Conflict)}
            let result=summary(&tx,&user.tenant.0,&upload.id)?;tx.commit()?;Ok(result)
        }).await
    }
    pub async fn media_library(&self, user: &UserContext) -> Result<MediaLibrary> {
        let user = user.clone();
        self.call(move|db|{
        let user=validate(&db.conn,&user,true,false)?;
        let items=db.conn.prepare(&format!("{SELECT} WHERE tenant_id=?1 AND account_id=?2 AND state!='deleted' ORDER BY created_at DESC,id LIMIT 128"))?.query_map(params![user.tenant.0,user.account.id],row)?.collect::<std::result::Result<Vec<_>,_>>()?;
        let used_bytes=db.conn.query_row("SELECT coalesce(sum(coalesce(length(bytes),0)+coalesce(length(thumbnail),0)+coalesce(length(CAST(extracted_text AS BLOB)),0)),0) FROM media_objects WHERE account_id=?1",[user.account.id],|r|r.get(0))?;
        Ok(MediaLibrary{items,used_bytes,quota_bytes:QUOTA_BYTES as u32})
    }).await
    }
    pub async fn media_bytes(
        &self,
        user: &UserContext,
        id: String,
        thumbnail: bool,
    ) -> Result<Vec<u8>> {
        let user = user.clone();
        self.call(move|db|{
        let user=validate(&db.conn,&user,true,false)?;
        db.conn.query_row(if thumbnail {"SELECT thumbnail FROM media_objects WHERE tenant_id=?1 AND account_id=?2 AND id=?3 AND state='ready' AND filename IS NULL"}else{"SELECT bytes FROM media_objects WHERE tenant_id=?1 AND account_id=?2 AND id=?3 AND state='ready' AND filename IS NULL"},params![user.tenant.0,user.account.id,id],|r|r.get(0)).optional()?.ok_or(StoreError::NotFound)
    }).await
    }
    pub async fn delete_media(&self, user: &UserContext, id: String) -> Result<()> {
        let user = user.clone();
        self.call(move |db| {
            let user = validate(&db.conn, &user, true, false)?;
            let id = Uuid::parse_str(&id)
                .map_err(|_| StoreError::InvalidInput)?
                .to_string();
            let owner: Option<String> = db
                .conn
                .query_row(
                    "SELECT account_id FROM media_objects WHERE tenant_id=?1 AND id=?2",
                    params![user.tenant.0, id],
                    |r| r.get(0),
                )
                .optional()?;
            if owner.as_ref().is_some_and(|v| v != &user.account.id) {
                return Err(StoreError::NotFound);
            }
            if owner.is_none() {
                let count: i64 = db.conn.query_row(
                    "SELECT count(*) FROM media_objects WHERE account_id=?1",
                    [&user.account.id],
                    |r| r.get(0),
                )?;
                if count >= 4096 {
                    return Err(StoreError::Busy);
                }
            }
            destructive_intents::commit(
                db,
                &user.tenant.0,
                destructive_intents::Effect::DeleteMedia {
                    target: id,
                    account: user.account.id,
                },
            )
        })
        .await
    }
}

/// Unpredictable synthetic proof pattern. Only exact image observation can pass.
pub fn vision_challenge() -> Result<(Vec<u8>, String)> {
    let mut random = [0u8; 16];
    OsRng
        .try_fill_bytes(&mut random)
        .map_err(|_| StoreError::Unavailable)?;
    let colors = [[255, 0, 0], [0, 255, 0], [0, 0, 255], [255, 255, 0]];
    let mut image = image::RgbImage::new(128, 128);
    let mut expected = String::new();
    for (i, v) in random.iter().enumerate() {
        let n = usize::from(v % 4);
        expected.push(['R', 'G', 'B', 'Y'][n]);
        for y in 0..32 {
            for x in 0..32 {
                image.put_pixel(
                    (i as u32 % 4) * 32 + x,
                    (i as u32 / 4) * 32 + y,
                    image::Rgb(colors[n]),
                );
            }
        }
    }
    let mut bytes = Vec::new();
    JpegEncoder::new_with_quality(&mut bytes, 95)
        .encode_image(&image)
        .map_err(|_| StoreError::Unavailable)?;
    Ok((bytes, expected))
}
impl Store {
    /// Trusted bounded server verifier only; never expose a client capability mark.
    pub async fn record_vision_probe(
        &self,
        user: &UserContext,
        id: String,
        revision: i64,
        model: String,
        effort: Option<String>,
    ) -> Result<()> {
        let user = user.clone();
        self.call(move|db|{let user=validate(&db.conn,&user,true,true)?;let _=user;let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
   let valid:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM providers p JOIN provider_revisions r ON r.provider_id=p.id AND r.revision=p.current_revision WHERE p.id=?1 AND p.current_revision=?2 AND r.enabled=1)",params![id,revision],|r|r.get(0))?;if !valid{return Err(StoreError::Conflict)}
   tx.execute("INSERT OR IGNORE INTO provider_vision_probes VALUES(?1,?2,?3,?4)",params![id,revision,model,effort.unwrap_or_default()])?;tx.commit()?;Ok(())
  }).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{TEST_LOCK, admin, directory, member};
    async fn image(store: &Store, user: &UserContext, id: &str) -> MediaSummary {
        let (bytes, _) = vision_challenge().unwrap();
        let digest = Sha256::digest(&bytes).into();
        let UploadAdmission::Process(upload) =
            store.media_begin(user, id.into(), digest).await.unwrap()
        else {
            panic!("new image")
        };
        store
            .media_finish(upload, Some(normalize(&bytes).unwrap()))
            .await
            .unwrap()
    }
    #[tokio::test]
    async fn immutable_receipt_quota_owner_and_reopen() {
        let _guard = TEST_LOCK.lock().await;
        let dir = directory();
        let store = Store::open(dir.path()).unwrap();
        let a = admin(&store).await.user;
        let b = member(&store, &a, "media-member@example.test").await.user;
        let id = Uuid::new_v4().to_string();
        let (bytes, _) = vision_challenge().unwrap();
        let digest: [u8; 32] = Sha256::digest(&bytes).into();
        let UploadAdmission::Process(upload) =
            store.media_begin(&a, id.clone(), digest).await.unwrap()
        else {
            panic!("new")
        };
        let ready = store
            .media_finish(upload, Some(normalize(&bytes).unwrap()))
            .await
            .unwrap();
        assert_eq!(ready.state, "ready");
        assert!(matches!(
            store.media_begin(&a, id.clone(), digest).await.unwrap(),
            UploadAdmission::Ready(_)
        ));
        assert!(matches!(
            store.media_begin(&a, id.clone(), [1; 32]).await,
            Err(StoreError::Conflict)
        ));
        assert!(matches!(
            store.media_bytes(&b, id.clone(), false).await,
            Err(StoreError::NotFound)
        ));
        let canonical = store.media_bytes(&a, id.clone(), false).await.unwrap();
        assert_ne!(canonical, bytes);
        let token = a.digest;
        drop(store);
        let store = Store::open(dir.path()).unwrap();
        let a = store
            .call(move |db| load_context(&db.conn, token, false))
            .await
            .unwrap();
        assert_eq!(store.media_bytes(&a, id, false).await.unwrap(), canonical);
        assert_eq!(store.media_library(&a).await.unwrap().items.len(), 1);
    }
    #[tokio::test]
    async fn tombstone_beats_both_early_admission_and_late_completion() {
        let _guard = TEST_LOCK.lock().await;
        let dir = directory();
        let store = Store::open(dir.path()).unwrap();
        let a = admin(&store).await.user;
        let (bytes, _) = vision_challenge().unwrap();
        let digest: [u8; 32] = Sha256::digest(&bytes).into();
        let early = Uuid::new_v4().to_string();
        store.delete_media(&a, early.clone()).await.unwrap();
        assert!(matches!(
            store.media_begin(&a, early, digest).await,
            Err(StoreError::Conflict)
        ));
        let late = Uuid::new_v4().to_string();
        let UploadAdmission::Process(upload) =
            store.media_begin(&a, late.clone(), digest).await.unwrap()
        else {
            panic!("new")
        };
        store.delete_media(&a, late.clone()).await.unwrap();
        assert!(matches!(
            store
                .media_finish(upload, Some(normalize(&bytes).unwrap()))
                .await,
            Err(StoreError::Conflict)
        ));
        assert!(matches!(
            store.media_bytes(&a, late, false).await,
            Err(StoreError::NotFound)
        ));
    }
    #[tokio::test]
    async fn quarantine_does_not_resume_processing_or_remove_ready_bytes() {
        let _guard = TEST_LOCK.lock().await;
        let dir = directory();
        let store = Store::open(dir.path()).unwrap();
        let a = admin(&store).await.user;
        let ready = Uuid::new_v4().to_string();
        image(&store, &a, &ready).await;
        let pending = Uuid::new_v4().to_string();
        store
            .media_begin(&a, pending.clone(), [3; 32])
            .await
            .unwrap();
        store.call(|db| quarantine(&db.conn)).await.unwrap();
        let library = store.media_library(&a).await.unwrap();
        assert_eq!(
            library
                .items
                .iter()
                .find(|v| v.id == pending)
                .unwrap()
                .state,
            "retry_required"
        );
        assert!(store.media_bytes(&a, ready, false).await.is_ok());
    }
    #[test]
    fn signatures_dimensions_and_decode_are_bounded() {
        assert!(normalize(b"<svg onload=alert(1)>").is_err());
        assert!(normalize(&vec![0; MAX_BYTES + 1]).is_err());
        assert!(normalize(b"\x89PNG\r\n\x1a\nmalformed").is_err());
        let image = image::RgbImage::new(2401, 1);
        let mut bytes = Vec::new();
        JpegEncoder::new_with_quality(&mut bytes, 60)
            .encode_image(&image)
            .unwrap();
        assert!(normalize(&bytes).is_err());
    }
}

#[cfg(test)]
mod fence_tests {
    use super::*;
    use crate::tests::{TEST_LOCK, admin, directory};
    use moosuri_core::{MediaRef, ModelSelection, ProviderConfig, ProviderKind};
    async fn configured(store: &Store, user: &UserContext) -> String {
        let provider = store
            .create_provider(
                user,
                ProviderChange {
                    name: "Synthetic media only".into(),
                    enabled: true,
                    credential: CredentialChange::Clear,
                    config: ProviderConfig {
                        kind: ProviderKind::Compatible,
                        base_url: Some("http://127.0.0.1:11434/v1".into()),
                        allowed_ips: vec!["127.0.0.1".into()],
                        router_providers: vec![],
                        zero_data_retention: false,
                    },
                },
            )
            .await
            .unwrap();
        store
            .record_model_probe(user, provider.id.clone(), 1, "synthetic-image".into(), None)
            .await
            .unwrap();
        store
            .record_vision_probe(user, provider.id.clone(), 1, "synthetic-image".into(), None)
            .await
            .unwrap();
        store
            .set_default_model(
                user,
                Some(ModelSelection {
                    source: Default::default(),
                    provider_id: provider.id.clone(),
                    provider_revision: 1,
                    model: "synthetic-image".into(),
                    effort: None,
                }),
            )
            .await
            .unwrap();
        provider.id
    }
    #[tokio::test]
    async fn pending_upload_does_not_lock_text_and_inherited_context_revokes() {
        let _guard = TEST_LOCK.lock().await;
        let dir = directory();
        let store = Store::open(dir.path()).unwrap();
        let user = admin(&store).await.user;
        configured(&store, &user).await;
        let thread = store.create_thread(&user, "media".into()).await.unwrap().id;
        let (bytes, _) = vision_challenge().unwrap();
        let digest = Sha256::digest(&bytes).into();
        let id = Uuid::new_v4().to_string();
        let UploadAdmission::Process(upload) =
            store.media_begin(&user, id.clone(), digest).await.unwrap()
        else {
            panic!("new")
        };
        let unrelated = store
            .submit_turn(
                &user,
                thread.clone(),
                Uuid::new_v4().to_string(),
                "text while uploading".into(),
            )
            .await
            .unwrap();
        assert!(unrelated.attachments.is_none());
        store.cancel_run(&user, unrelated.run.id, 1).await.unwrap();
        store
            .media_finish(upload, Some(normalize(&bytes).unwrap()))
            .await
            .unwrap();
        let refs = vec![MediaRef {
            id: id.clone(),
            revision: 1,
        }];
        let request = Uuid::new_v4().to_string();
        let first = store
            .submit_turn_with_media(
                &user,
                thread.clone(),
                request.clone(),
                "image".into(),
                None,
                refs.clone(),
            )
            .await
            .unwrap();
        let duplicate = store
            .submit_turn_with_media(
                &user,
                thread.clone(),
                request.clone(),
                "image".into(),
                None,
                refs,
            )
            .await
            .unwrap();
        assert_eq!(first.run.id, duplicate.run.id);
        assert!(matches!(
            store
                .submit_turn_with_media(
                    &user,
                    thread.clone(),
                    request,
                    "image".into(),
                    None,
                    vec![]
                )
                .await,
            Err(StoreError::Conflict)
        ));
        store.cancel_run(&user, first.run.id, 1).await.unwrap();
        let next = store
            .submit_turn(
                &user,
                thread,
                Uuid::new_v4().to_string(),
                "followup retains conservative provenance".into(),
            )
            .await
            .unwrap();
        let tenant = user.tenant.0.clone();
        let run = next.run.id.clone();
        assert!(
            store
                .call(move |db| has_context(&db.conn, &tenant, &run))
                .await
                .unwrap()
        );
        store.delete_media(&user, id).await.unwrap();
        let page = store
            .conversation(&user, next.run.thread_id, None)
            .await
            .unwrap();
        assert_eq!(page.turns[0].run.state, moosuri_core::RunState::Cancelled);
        assert!(
            page.turns[1..]
                .iter()
                .all(|t| !t.run.can_retry && !t.run.state.active())
        );
    }
    #[tokio::test]
    async fn changed_auth_can_reclaim_processing_and_old_completion_is_fenced() {
        let _guard = TEST_LOCK.lock().await;
        let dir = directory();
        let store = Store::open(dir.path()).unwrap();
        let user = admin(&store).await.user;
        let (bytes, _) = vision_challenge().unwrap();
        let digest = Sha256::digest(&bytes).into();
        let id = Uuid::new_v4().to_string();
        let UploadAdmission::Process(old) =
            store.media_begin(&user, id.clone(), digest).await.unwrap()
        else {
            panic!("new")
        };
        let changed = store
            .change_password(&user, "Synthetic-media-new-password-42".into())
            .await
            .unwrap();
        let UploadAdmission::Process(new) = store
            .media_begin(&changed.user, id.clone(), digest)
            .await
            .unwrap()
        else {
            panic!("reclaim")
        };
        assert!(matches!(
            store
                .media_finish(old, Some(normalize(&bytes).unwrap()))
                .await,
            Err(StoreError::Unauthorized)
        ));
        assert_eq!(
            store
                .media_finish(new, Some(normalize(&bytes).unwrap()))
                .await
                .unwrap()
                .state,
            "ready"
        );
    }
}

#[cfg(test)]
mod codec_quota_tests {
    use super::*;
    use crate::tests::{TEST_LOCK, admin, directory};
    #[test]
    fn orientation_is_applied_and_private_exif_is_removed() {
        let image = image::RgbImage::new(80, 40);
        let mut jpeg = Vec::new();
        JpegEncoder::new_with_quality(&mut jpeg, 75)
            .encode_image(&image)
            .unwrap();
        let mut exif = b"Exif\0\0II".to_vec();
        exif.extend(42u16.to_le_bytes());
        exif.extend(8u32.to_le_bytes());
        exif.extend(1u16.to_le_bytes());
        exif.extend(0x112u16.to_le_bytes());
        exif.extend(3u16.to_le_bytes());
        exif.extend(1u32.to_le_bytes());
        exif.extend(6u16.to_le_bytes());
        exif.extend(0u16.to_le_bytes());
        exif.extend(0u32.to_le_bytes());
        let marker = b"synthetic-private-exif-must-not-survive";
        exif.extend(marker);
        let mut input = jpeg[..2].to_vec();
        input.extend([255, 225]);
        input.extend(((exif.len() + 2) as u16).to_be_bytes());
        input.extend(exif);
        input.extend(&jpeg[2..]);
        let ready = normalize(&input).unwrap();
        assert_eq!((ready.width, ready.height), (40, 80));
        assert!(!ready.bytes.windows(marker.len()).any(|v| v == marker));
        assert!(!ready.thumbnail.windows(marker.len()).any(|v| v == marker));
    }
    #[tokio::test]
    async fn pending_reservations_include_thumbnail_headroom_in_account_quota() {
        let _guard = TEST_LOCK.lock().await;
        let dir = directory();
        let store = Store::open(dir.path()).unwrap();
        let user = admin(&store).await.user;
        for _ in 0..30 {
            assert!(matches!(
                store
                    .media_begin(&user, Uuid::new_v4().to_string(), [4; 32])
                    .await
                    .unwrap(),
                UploadAdmission::Process(_)
            ));
        }
        assert!(matches!(
            store
                .media_begin(&user, Uuid::new_v4().to_string(), [4; 32])
                .await,
            Err(StoreError::Busy)
        ));
    }
}

#[cfg(test)]
mod replay_tests {
    use super::*;
    use crate::tests::{TEST_LOCK, admin, directory};
    #[tokio::test]
    async fn newer_delete_journal_withdraws_ready_blob_from_an_older_sqlite_snapshot() {
        let _guard = TEST_LOCK.lock().await;
        let dir = directory();
        let snapshot = directory();
        let saved = snapshot.path().join("snapshot.sqlite");
        let store = Store::open(dir.path()).unwrap();
        let user = admin(&store).await.user;
        let (bytes, _) = vision_challenge().unwrap();
        let digest = Sha256::digest(&bytes).into();
        let id = Uuid::new_v4().to_string();
        let UploadAdmission::Process(upload) =
            store.media_begin(&user, id.clone(), digest).await.unwrap()
        else {
            panic!("new")
        };
        store
            .media_finish(upload, Some(normalize(&bytes).unwrap()))
            .await
            .unwrap();
        let path = saved.clone();
        store
            .call(move |db| {
                let mut destination = Connection::open(path)?;
                let backup = rusqlite::backup::Backup::new(&db.conn, &mut destination)?;
                backup.run_to_completion(128, std::time::Duration::from_millis(1), None)?;
                Ok(())
            })
            .await
            .unwrap();
        store.delete_media(&user, id.clone()).await.unwrap();
        drop(store);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&saved, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        std::fs::copy(&saved, dir.path().join("assistant.sqlite")).unwrap();
        let restored = Store::open(dir.path()).unwrap();
        assert!(matches!(
            restored.media_bytes(&user, id, false).await,
            Err(StoreError::NotFound)
        ));
        assert!(
            restored
                .media_library(&user)
                .await
                .unwrap()
                .items
                .is_empty()
        );
    }
}

#[cfg(test)]
mod tombstone_quota_tests {
    use super::*;
    use crate::tests::{TEST_LOCK, admin, directory};
    #[tokio::test]
    async fn unknown_id_cancellation_cannot_bypass_the_account_id_quota() {
        let _guard = TEST_LOCK.lock().await;
        let dir = directory();
        let store = Store::open(dir.path()).unwrap();
        let user = admin(&store).await.user;
        let owner = user.clone();
        store.call(move|db|{let tx=db.conn.transaction()?;for _ in 0..4096{tx.execute("INSERT INTO media_objects(tenant_id,account_id,id,state,auth_epoch,created_at) VALUES(?1,?2,?3,'deleted',0,0)",params![owner.tenant.0,owner.account.id,Uuid::new_v4().to_string()])?;}tx.commit()?;Ok(())}).await.unwrap();
        assert!(matches!(
            store.delete_media(&user, Uuid::new_v4().to_string()).await,
            Err(StoreError::Busy)
        ));
        store
            .call(|db| {
                assert!(db.destructive_journal.records.is_empty());
                Ok(())
            })
            .await
            .unwrap();
    }
    #[tokio::test]
    async fn migration_failure_rolls_back_partial_image_tables_and_can_retry() {
        let _guard = TEST_LOCK.lock().await;
        let dir = directory();
        let store = Store::open(dir.path()).unwrap();
        let user = admin(&store).await.user;
        drop(store);
        let conn = Connection::open(dir.path().join("assistant.sqlite")).unwrap();
        conn.execute_batch("DROP INDEX local_notifications_source;DROP TRIGGER notification_suppression_monotonic;DROP TABLE notification_source_preferences;ALTER TABLE local_notifications DROP COLUMN source_suppressed;ALTER TABLE local_notifications DROP COLUMN push_suppressed;DROP TABLE learned_skill_receipts;DROP TABLE learned_skill_observations;DROP TABLE learned_skill_uses;DROP TABLE learned_skill_proofs;DROP TABLE learned_skills;DROP TABLE credential_broker_auth;DROP TABLE credential_broker_bindings;DROP TABLE credential_broker_devices;DROP TABLE credential_broker_pairs;DROP TABLE credential_broker_invocations;DROP TABLE credential_broker_commitments;DROP TABLE community_catalog_cache;DROP TRIGGER chatgpt_handoff_session_logout;DROP TRIGGER chatgpt_handoff_account_epoch;DROP TRIGGER chatgpt_handoff_binding_change;DROP TRIGGER session_purpose_valid_insert;DROP TRIGGER session_purpose_immutable;ALTER TABLE sessions DROP COLUMN absolute_deadline;ALTER TABLE sessions DROP COLUMN purpose;DROP TABLE chatgpt_handoffs;DROP TRIGGER audio_authority_no_delete;DROP TRIGGER audio_authority_monotonic;DROP TABLE audio_jobs;DROP TABLE audio_selection;DROP TABLE audio_authority;DROP TRIGGER personal_subscription_exclusive;DROP TRIGGER personal_shared_exclusive;DROP TRIGGER personal_shared_exclusive_update;DROP TABLE personal_subscription_selections;DROP TABLE chatgpt_model_probes;DROP TABLE chatgpt_auth_attempts;DROP TABLE chatgpt_sessions;DROP TABLE chatgpt_binding_revisions;DROP TABLE chatgpt_bindings;DROP TABLE subscription_runtime;DROP TABLE mail_reconciliations;DROP TABLE mail_send_evidence;DROP TABLE calendar_agent_action_origins;DROP TABLE calendar_agent_action_links;DROP TABLE calendar_command_plans;DROP TABLE calendar_agent_observations;DROP TABLE calendar_run_dependencies;DROP TABLE calendar_agent_profiles;DROP TABLE mission_chat_commands;DROP TABLE mission_chat_plans;DROP TABLE recurring_mission_occurrences;DROP TABLE recurring_missions;DROP TABLE calendar_actions;DROP TABLE calendar_oauth_sessions;DROP TABLE calendar_connections;DROP TABLE mission_controls;DROP TABLE mission_revisions;DROP TABLE missions;ALTER TABLE threads DROP COLUMN mission_internal;DROP TABLE vault_handoffs;DROP TABLE vault_grants;DROP TABLE vault_operation_commitments;DROP TABLE push_enrollment_receipts;DROP TABLE push_queue;DROP TABLE push_subscriptions;DROP TABLE push_vapid_meta;DROP TABLE push_owners;DROP TABLE push_worker_state;DROP TABLE schedule_chat_commands;DROP TABLE chat_media; DROP TABLE media_run_dependencies; DROP TABLE media_objects; DROP TABLE provider_vision_probes; CREATE TABLE provider_vision_probes(conflict INTEGER); PRAGMA user_version=20;").unwrap();
        drop(conn);
        assert!(Store::open(dir.path()).is_err());
        let conn = Connection::open(dir.path().join("assistant.sqlite")).unwrap();
        assert_eq!(
            conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            20
        );
        assert_eq!(conn.query_row("SELECT count(*) FROM sqlite_schema WHERE type='table' AND name IN('media_objects','chat_media','media_run_dependencies')", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
        conn.execute_batch("DROP TABLE provider_vision_probes;")
            .unwrap();
        drop(conn);
        let store = Store::open(dir.path()).unwrap();
        assert!(store.media_library(&user).await.unwrap().items.is_empty());
    }
}
