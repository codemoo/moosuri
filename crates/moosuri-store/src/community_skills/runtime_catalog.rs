//! Public cache and discovery only. User installations remain in existing lifecycle.
use super::*;
use moosuri_connectors::{
    github_catalog::{CatalogIndex, CatalogItem, CatalogSource, IndexResponse},
    github_skills::DownloadedGitHubSkill,
};
use moosuri_core::{CommunityCatalogOffer, CommunityCatalogStatus};

pub struct CatalogRefresh {
    pub etag: Option<String>,
    source: CatalogSource,
    nonce: String,
    generation: i64,
}
pub struct CatalogPreview {
    pub source: CatalogSource,
    pub item: CatalogItem,
    pub repository: String,
    pub commit: String,
    generation: i64,
}
struct Cache {
    json: Option<String>,
    digest: Option<String>,
    etag: Option<String>,
    verified: Option<i64>,
    checked: i64,
    error: Option<String>,
    generation: i64,
    nonce: Option<String>,
    until: Option<i64>,
}
fn cache(c: &Connection, s: &CatalogSource) -> Result<Option<Cache>> {
    c.query_row("SELECT index_json,index_digest,etag,verified_at,checked_at,error,generation,lease_nonce,lease_until FROM community_catalog_cache WHERE source_key=?1 AND source=?2",params![s.key(),serde_json::to_string(s).map_err(|_|StoreError::Corrupt)?],|r|Ok(Cache{json:r.get(0)?,digest:r.get(1)?,etag:r.get(2)?,verified:r.get(3)?,checked:r.get(4)?,error:r.get(5)?,generation:r.get(6)?,nonce:r.get(7)?,until:r.get(8)?})).optional().map_err(Into::into)
}
fn verified_index(r: &Cache, s: &CatalogSource) -> Result<CatalogIndex> {
    let bytes = r.json.as_ref().ok_or(StoreError::NotFound)?;
    if r.digest.as_deref() != Some(format!("{:x}", Sha256::digest(bytes.as_bytes())).as_str()) {
        return Err(StoreError::Corrupt);
    }
    CatalogIndex::parse(bytes.as_bytes(), s).map_err(|_| StoreError::Corrupt)
}
fn item_matches(e: &Validated, index: &CatalogIndex, item: &CatalogItem) -> Result<()> {
    if e.external
        .as_ref()
        .is_none_or(|x| x.repository != index.repository || x.directory != item.directory)
        || e.id() != item.id
        || e.package.commit != index.source_commit
        || e.digest != item.content_digest
        || e.package.slug != item.name
        || e.metadata.version != item.version
        || e.description != item.description
        || e.metadata.license != item.license
        || e.metadata.capabilities != item.capabilities
    {
        return Err(StoreError::InvalidInput);
    }
    Ok(())
}
fn merged(
    c: &Connection,
    u: &UserContext,
    web: &moosuri_core::WebToolProfile,
    s: &CatalogSource,
) -> Result<Vec<CommunitySkillCard>> {
    let mut result = cards(c, u, web)?;
    let Some(r) = cache(c, s)? else {
        return Ok(result);
    };
    let Ok(index) = verified_index(&r, s) else {
        return Ok(result);
    };
    for item in &index.packages {
        let row=c.query_row("SELECT active_digest,previous_digest,enabled,hidden,revision FROM community_installs WHERE tenant_id=?1 AND package_id=?2",params![u.tenant.0,item.id],|r|Ok((r.get::<_,Option<String>>(0)?,r.get::<_,Option<String>>(1)?,r.get::<_,bool>(2)?,r.get::<_,bool>(3)?,r.get::<_,i64>(4)?))).optional()?;
        let (active, previous, enabled, hidden, revision) =
            row.unwrap_or((None, None, false, false, 0));
        // An existing immutable snapshot was staged through verified GitHub acquisition,
        // never through an index claim. Recheck its exact source/digest/metadata.
        let snapshot = stored(c, &item.content_digest)
            .ok()
            .filter(|e| item_matches(e, &index, item).is_ok());
        let mut offer = if let Some(entry) = snapshot {
            card(c, u, &entry, web, false)?
        } else {
            let installed = active.as_deref().map(|d| stored(c, d)).transpose()?;
            CommunitySkillCard {
                id: item.id.clone(),
                name: item.name.clone(),
                description: item.description.clone(),
                source: format!(
                    "{}/tree/{}/{}",
                    index.repository, index.source_commit, item.directory
                ),
                source_commit: index.source_commit.clone(),
                version: item.version.clone(),
                digest: item.content_digest.clone(),
                license: item.license.clone(),
                capabilities: item.capabilities.clone(),
                installed_version: installed.as_ref().map(|e| e.metadata.version.clone()),
                installed_digest: active,
                enabled,
                hidden,
                revision,
                can_rollback: previous.is_some(),
                readiness: if installed.is_some() {
                    "catalog_preview_required"
                } else {
                    "preview_required"
                }
                .into(),
                missing: Vec::new(),
                github: Some(moosuri_core::GitHubSkillLocation {
                    repository: index.repository.clone(),
                    directory: item.directory.clone(),
                }),
                catalog: None,
            }
        };
        offer.catalog = Some(CommunityCatalogOffer {
            artifact_sha256: item.artifact_sha256.clone(),
            stale: r.error.is_some()
                || r.verified
                    .is_none_or(|t| t + 86400 <= now().unwrap_or(i64::MAX)),
            verified: offer.readiness != "preview_required"
                && offer.readiness != "catalog_preview_required",
        });
        // Keep an installed prior version usable until explicit verified update.
        // New-index preview must never suppress its current readiness/start action.
        if let Some(existing) = result.iter_mut().find(|c| c.id == item.id) {
            if !offer.catalog.as_ref().is_some_and(|c| c.verified) {
                existing.catalog = offer.catalog;
                existing.digest = offer.digest;
                existing.version = offer.version;
                existing.source_commit = offer.source_commit;
                existing.source = offer.source;
            } else {
                *existing = offer;
            }
        } else {
            result.push(offer);
        }
    }
    Ok(result)
}
impl Store {
    pub async fn community_catalog_status(
        &self,
        u: &UserContext,
        s: Option<CatalogSource>,
    ) -> Result<CommunityCatalogStatus> {
        let u = u.clone();
        self.call(move |db| {
            let _ = validate(&db.conn, &u, true, false)?;
            let Some(s) = s else {
                return Ok(CommunityCatalogStatus {
                    configured: false,
                    source: None,
                    state: "disabled".into(),
                    verified_at: None,
                    checked_at: None,
                    stale: false,
                });
            };
            let r = cache(&db.conn, &s)?;
            let valid = r.as_ref().is_some_and(|r| verified_index(r, &s).is_ok());
            let stale = r.as_ref().is_none_or(|r| {
                !valid
                    || r.error.is_some()
                    || r.verified
                        .is_none_or(|t| t + 86400 <= now().unwrap_or(i64::MAX))
            });
            Ok(CommunityCatalogStatus {
                configured: true,
                source: Some(s.label()),
                state: if r.as_ref().is_some_and(|r| r.json.is_some()) && !valid {
                    "corrupt"
                } else if r.as_ref().is_some_and(|r| {
                    r.nonce.is_some() && r.until.is_some_and(|t| t > now().unwrap_or(i64::MAX))
                }) {
                    "checking"
                } else if r.as_ref().is_some_and(|r| r.error.is_some()) {
                    if valid {
                        "offline_cache"
                    } else {
                        "unavailable"
                    }
                } else if valid {
                    "verified_index"
                } else {
                    "not_checked"
                }
                .into(),
                verified_at: r.as_ref().filter(|_| valid).and_then(|r| r.verified),
                checked_at: r.map(|r| r.checked).filter(|t| *t > 0),
                stale,
            })
        })
        .await
    }
    pub async fn community_catalog_cards(
        &self,
        u: &UserContext,
        s: CatalogSource,
    ) -> Result<Vec<CommunitySkillCard>> {
        let u = u.clone();
        self.call(move |db| {
            let u = validate(&db.conn, &u, true, false)?;
            merged(&db.conn, &u, &db.web_profile, &s)
        })
        .await
    }
    pub async fn begin_community_catalog_refresh(
        &self,
        u: &UserContext,
        s: CatalogSource,
        manual: bool,
    ) -> Result<Option<CatalogRefresh>> {
        let u = u.clone();
        self.call(move|db|{
            validate(&db.conn,&u,true,false)?;let time=now()?;
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            tx.execute("INSERT OR IGNORE INTO community_catalog_cache(source_key,source) VALUES(?1,?2)",params![s.key(),serde_json::to_string(&s).map_err(|_|StoreError::Corrupt)?])?;
            let r=cache(&tx,&s)?.ok_or(StoreError::Corrupt)?;let valid=verified_index(&r,&s).is_ok();
            if r.until.is_some_and(|t|t>time) || r.checked>0 && r.checked+60>time || !manual&&valid&&r.checked+86400>time {return Ok(None);}
            let nonce=Uuid::new_v4().to_string();tx.execute("UPDATE community_catalog_cache SET checked_at=?2,lease_nonce=?3,lease_until=?4 WHERE source_key=?1",params![s.key(),time,nonce,time+45])?;tx.commit()?;
            Ok(Some(CatalogRefresh{etag:if valid{r.etag}else{None},source:s,nonce,generation:r.generation}))
        }).await
    }
    pub async fn complete_community_catalog_refresh(
        &self,
        u: &UserContext,
        l: CatalogRefresh,
        response: std::result::Result<IndexResponse, moosuri_connectors::github_catalog::Error>,
    ) -> Result<()> {
        let u = u.clone();
        self.call(move|db|{
            validate(&db.conn,&u,true,false)?;let r=cache(&db.conn,&l.source)?.ok_or(StoreError::Conflict)?;
            if r.nonce.as_ref()!=Some(&l.nonce)||r.generation!=l.generation||r.until.is_none_or(|t|t<=now().unwrap_or(i64::MAX)){return Err(StoreError::Conflict);}
            let validated=match response {
                Ok(IndexResponse::Modified{bytes,etag})=>CatalogIndex::parse(&bytes,&l.source).ok().map(|_|(String::from_utf8(bytes).ok(),etag)),
                Ok(IndexResponse::NotModified) if l.etag.is_some()&&verified_index(&r,&l.source).is_ok()=>Some((r.json.clone(),r.etag.clone())),_=>None,
            };
            if let Some((Some(json),etag))=validated {
                let digest=format!("{:x}",Sha256::digest(json.as_bytes()));db.conn.execute("UPDATE community_catalog_cache SET index_json=?2,index_digest=?3,etag=?4,verified_at=?5,error=NULL,generation=generation+1,lease_nonce=NULL,lease_until=NULL WHERE source_key=?1",params![l.source.key(),json,digest,etag,now()?])?;
            }else{db.conn.execute("UPDATE community_catalog_cache SET error='catalog_unavailable',lease_nonce=NULL,lease_until=NULL WHERE source_key=?1",[l.source.key()])?;}
            Ok(())
        }).await
    }
    pub async fn community_catalog_preview(
        &self,
        u: &UserContext,
        s: CatalogSource,
        id: String,
        digest: String,
    ) -> Result<CatalogPreview> {
        let u = u.clone();
        self.call(move |db| {
            validate(&db.conn, &u, true, false)?;
            let r = cache(&db.conn, &s)?.ok_or(StoreError::NotFound)?;
            let index = verified_index(&r, &s)?;
            let item = index
                .packages
                .iter()
                .find(|i| i.id == id && i.content_digest == digest)
                .cloned()
                .ok_or(StoreError::Conflict)?;
            Ok(CatalogPreview {
                source: s,
                item,
                repository: index.repository,
                commit: index.source_commit,
                generation: r.generation,
            })
        })
        .await
    }
    pub async fn reuse_community_catalog_preview(
        &self,
        u: &UserContext,
        p: &CatalogPreview,
    ) -> Result<Option<CommunitySkillCard>> {
        let (u, s, item, generation) = (u.clone(), p.source.clone(), p.item.clone(), p.generation);
        self.call(move |db| {
            let u = validate(&db.conn, &u, true, false)?;
            let r = cache(&db.conn, &s)?.ok_or(StoreError::Conflict)?;
            if r.generation != generation {
                return Err(StoreError::Conflict);
            }
            let index = verified_index(&r, &s)?;
            let exists: bool = db.conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM community_packages WHERE digest=?1)",
                [&item.content_digest],
                |r| r.get(0),
            )?;
            if !exists {
                return Ok(None);
            }
            let e = stored(&db.conn, &item.content_digest)?;
            item_matches(&e, &index, &item)?;
            // This replaces the discovery card, so readiness belongs to the
            // active installation until the user explicitly updates it.
            let mut c = card(&db.conn, &u, &e, &db.web_profile, false)?;
            c.catalog = Some(CommunityCatalogOffer {
                artifact_sha256: item.artifact_sha256,
                verified: true,
                stale: r.error.is_some(),
            });
            Ok(Some(c))
        })
        .await
    }
    pub async fn complete_community_catalog_preview(
        &self,
        u: &UserContext,
        p: CatalogPreview,
        artifact: Vec<u8>,
        downloaded: DownloadedGitHubSkill,
    ) -> Result<CommunitySkillCard> {
        // Preserve existing origin authority: independently acquired exact public
        // GitHub bytes, not artifact labels, construct the installed carrier.
        let entry = external::downloaded(downloaded)?;
        if format!("{:x}", Sha256::digest(&artifact)) != p.item.artifact_sha256 {
            return Err(StoreError::InvalidInput);
        }
        let artifact_entry = external::decode_catalog_artifact(&artifact)?;
        if external::encode(&entry)? != external::encode(&artifact_entry)? {
            return Err(StoreError::InvalidInput);
        }
        let u = u.clone();
        self.call(move |db| {
            let u = validate(&db.conn, &u, true, false)?;
            let r = cache(&db.conn, &p.source)?.ok_or(StoreError::Conflict)?;
            if r.generation != p.generation {
                return Err(StoreError::Conflict);
            }
            let index = verified_index(&r, &p.source)?;
            item_matches(&entry, &index, &p.item)?;
            stage(&db.conn, &entry)?;
            let mut c = card(&db.conn, &u, &entry, &db.web_profile, false)?;
            c.catalog = Some(CommunityCatalogOffer {
                artifact_sha256: p.item.artifact_sha256,
                verified: true,
                stale: r.error.is_some(),
            });
            Ok(c)
        })
        .await
    }
    pub async fn set_community_catalog_hidden(
        &self,
        u: &UserContext,
        s: CatalogSource,
        id: String,
        digest: String,
        revision: i64,
        hidden: bool,
    ) -> Result<()> {
        let u = u.clone();
        self.call(move|db|{
            let u=validate(&db.conn,&u,true,false)?;let r=cache(&db.conn,&s)?.ok_or(StoreError::NotFound)?;let index=verified_index(&r,&s)?;if !index.packages.iter().any(|i|i.id==id&&i.content_digest==digest){return Err(StoreError::Conflict);}
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let current:Option<i64>=tx.query_row("SELECT revision FROM community_installs WHERE tenant_id=?1 AND package_id=?2",params![u.tenant.0,id],|r|r.get(0)).optional()?;
            if current.unwrap_or(0)!=revision{return Err(StoreError::Conflict);}
            if current.is_none()&&tx.query_row("SELECT count(*) FROM community_installs WHERE tenant_id=?1 AND package_id LIKE 'github-%'",[&u.tenant.0],|r|r.get::<_,i64>(0))?>=64{return Err(StoreError::BudgetExhausted);}
            tx.execute("INSERT INTO community_installs(tenant_id,package_id,hidden,revision) VALUES(?1,?2,?3,1) ON CONFLICT(tenant_id,package_id) DO UPDATE SET hidden=excluded.hidden,revision=community_installs.revision+1",params![u.tenant.0,id,hidden])?;tx.commit()?;Ok(())
        }).await
    }
}
#[cfg(test)]
mod tests;
