//! No-progress detection for one private browser task. The verdict compares what was actually read
//! (title, visible text) and the amount of later guidance, never UI wording.
//! A row exists only for a run of the current member volume and is removed with its job.
use super::*;

/// Worklog/view reason for a task paused because another read cannot add anything new.
pub const NO_PROGRESS: &str = "browser_no_progress";
const GUIDED: &str = "new_guidance";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Progress {
    /// New page/selection content, or new instructions since the previous observation.
    Advanced,
    /// Identical content with identical instructions as the previous observation of this run.
    Repeated,
}

/// Stable digest of readable content. Whitespace layout is irrelevant; title and text are not.
/// The address is deliberately excluded: the same document under another address adds no evidence.
pub fn fingerprint(title: &str, text: &str, links: &[(&str, &str)]) -> String {
    let mut bytes = Vec::new();
    // A reordered menu is the same choice set. A newly available document is progress.
    let mut choices = links.to_vec();
    choices.sort_unstable();
    choices.dedup();
    for part in [title, text].into_iter().chain(
        choices
            .into_iter()
            .flat_map(|(target, label)| [target, label]),
    ) {
        let normalized = part.split_whitespace().collect::<Vec<_>>().join(" ");
        bytes.extend((normalized.len() as u64).to_be_bytes());
        bytes.extend(normalized.as_bytes());
    }
    format!("{:x}", Sha256::digest(&bytes))
}

/// A person or the next explicit read resolves the pause; the stale reason must not outlive it.
pub(super) fn release(view: &mut Workspace) {
    if view.reason.as_deref() == Some(NO_PROGRESS) {
        view.reason = None;
    }
}

/// Instructions arriving while paused turn the pause into the ordinary "needs a new read" reason.
pub(super) fn guided(c: &Connection, scope: &Scope) -> Result<()> {
    let job = current(c, scope)?.ok_or(StoreError::NotFound)?;
    if job.view.state == "ready"
        && job.pending.is_none()
        && job.view.reason.as_deref() == Some(NO_PROGRESS)
    {
        let mut view = job.view;
        view.reason = Some(GUIDED.into());
        view.revision += 1;
        save(c, scope, &view)?;
    }
    Ok(())
}

impl Store {
    /// Record this execution's observation. It is bound to the dispatch (`request`) that is still
    /// pending, so a stale or replayed loop cannot write for a newer controller or another run.
    pub async fn personal_browser_progress(
        &self,
        scope: Scope,
        request: Uuid,
        workspace_id: String,
        fingerprint: String,
    ) -> Result<Progress> {
        if fingerprint.len() != 64 || !fingerprint.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(StoreError::InvalidInput);
        }
        self.call(move |db| {
            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            let job = current(&tx, &scope)?.ok_or(StoreError::NotFound)?;
            if job.view.id != workspace_id || job.pending != Some(request.to_string()) {
                return Err(StoreError::Conflict);
            }
            let (tenant, run) = (&scope.user.tenant.0, &job.view.run_id);
            let guidance = job.guidance.len() as i64;
            let old: Option<(String, i64, i64)> = tx
                .query_row(
                    "SELECT fingerprint,guidance,repeats FROM personal_browser_progress WHERE tenant_id=?1 AND run_id=?2",
                    params![tenant, run],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()?;
            let verdict = match old {
                Some((previous, count, repeats)) if previous == fingerprint && count == guidance => {
                    tx.execute(
                        "UPDATE personal_browser_progress SET repeats=?3 WHERE tenant_id=?1 AND run_id=?2",
                        params![tenant, run, repeats.saturating_add(1)],
                    )?;
                    Progress::Repeated
                }
                _ => {
                    tx.execute(
                        "INSERT INTO personal_browser_progress(tenant_id,run_id,fingerprint,guidance,repeats) VALUES(?1,?2,?3,?4,0)
                         ON CONFLICT(tenant_id,run_id) DO UPDATE SET fingerprint=excluded.fingerprint,guidance=excluded.guidance,repeats=0",
                        params![tenant, run, fingerprint, guidance],
                    )?;
                    Progress::Advanced
                }
            };
            tx.commit()?;
            Ok(verdict)
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn member(name: &str) -> (Store, VolumeOwner, UserContext, tempfile::TempDir) {
        let dir = crate::tests::directory();
        let root = dir.path().canonicalize().unwrap();
        let owner = VolumeOwner {
            account: Uuid::new_v4(),
            tenant: Uuid::new_v4(),
            volume: Uuid::new_v4(),
            generation: 1,
        };
        let email = format!("{name}@example.test");
        owner
            .initialize(
                &root,
                "Member".into(),
                email.clone(),
                "Temporary-progress-42".into(),
            )
            .await
            .unwrap();
        let store = owner.open_verified(&root).await.unwrap();
        let login = store
            .login(email, "Temporary-progress-42".into())
            .await
            .unwrap();
        let user = store
            .change_password(&login.user, "Changed-progress-password-42".into())
            .await
            .unwrap()
            .user;
        (store, owner, user, dir)
    }
    async fn opened(
        store: &Store,
        owner: VolumeOwner,
        user: &UserContext,
        title: &str,
    ) -> (Scope, Uuid, Workspace) {
        let thread = store.create_thread(user, title.into()).await.unwrap().id;
        let scope = Scope {
            owner,
            user: user.clone(),
            thread,
        };
        let request = Uuid::new_v4();
        let Admission::Fresh(job) = store
            .personal_browser_open(
                scope.clone(),
                request,
                "https://example.org/".into(),
                "Read it".into(),
                false,
            )
            .await
            .unwrap()
        else {
            panic!("expected fresh")
        };
        (scope, request, job.view)
    }

    #[test]
    fn fingerprint_follows_content_not_layout_or_ui_wording() {
        let a = fingerprint("Timetable", "Morning bus  08:15\n", &[]);
        assert_eq!(a, fingerprint("Timetable", "Morning bus 08:15", &[]));
        assert_ne!(a, fingerprint("Timetable", "Morning bus 09:15", &[]));
        assert_ne!(
            fingerprint("a", "bc", &[]),
            fingerprint("ab", "c", &[]),
            "field boundaries are part of the digest"
        );
    }

    #[tokio::test]
    async fn progress_pauses_only_identical_reads_and_is_isolated_per_run() {
        let _guard = crate::tests::TEST_LOCK.lock().await;
        let (store, owner, user, _dir) = member("progress").await;
        let (scope, request, view) = opened(&store, owner, &user, "First").await;
        let (other, other_request, other_view) = opened(&store, owner, &user, "Second").await;
        let a = fingerprint("T", "one", &[]);
        let b = fingerprint("T", "two", &[]);
        let observe = |scope: &Scope, request: Uuid, view: &Workspace, fp: &String| {
            store.personal_browser_progress(scope.clone(), request, view.id.clone(), fp.clone())
        };
        assert_eq!(
            observe(&scope, request, &view, &a).await.unwrap(),
            Progress::Advanced
        );
        assert_eq!(
            observe(&scope, request, &view, &a).await.unwrap(),
            Progress::Repeated
        );
        // The other run has not read this content yet; its own history decides.
        assert_eq!(
            observe(&other, other_request, &other_view, &a)
                .await
                .unwrap(),
            Progress::Advanced
        );
        // Genuinely changed content continues regardless of the message shown to the person.
        assert_eq!(
            observe(&scope, request, &view, &b).await.unwrap(),
            Progress::Advanced
        );
        assert_eq!(
            observe(&scope, request, &view, &b).await.unwrap(),
            Progress::Repeated
        );
        // New instructions make the same page worth reading again.
        store
            .personal_browser_guide(
                scope.clone(),
                view.id.clone(),
                Uuid::new_v4(),
                "only mornings".into(),
            )
            .await
            .unwrap();
        assert_eq!(
            observe(&scope, request, &view, &b).await.unwrap(),
            Progress::Advanced
        );
        // A stale dispatch, a foreign workspace and malformed digests write nothing.
        assert!(matches!(
            observe(&scope, Uuid::new_v4(), &view, &a).await,
            Err(StoreError::Conflict)
        ));
        assert!(matches!(
            observe(&scope, request, &other_view, &a).await,
            Err(StoreError::Conflict)
        ));
        assert!(matches!(
            observe(&scope, request, &view, &"zz".into()).await,
            Err(StoreError::InvalidInput)
        ));
        assert_eq!(
            observe(&scope, request, &view, &b).await.unwrap(),
            Progress::Repeated
        );
    }

    #[tokio::test]
    async fn pause_reason_survives_reload_turns_into_guidance_and_clears_on_control() {
        let _guard = crate::tests::TEST_LOCK.lock().await;
        let (store, owner, user, _dir) = member("pause").await;
        let (scope, request, mut view) = opened(&store, owner, &user, "Paused").await;
        view.state = "ready".into();
        view.reason = Some(NO_PROGRESS.into());
        let finished = store
            .personal_browser_finish(
                scope.clone(),
                view,
                request,
                Some(Output::Observation("Paused".into())),
            )
            .await
            .unwrap();
        let reloaded = store
            .personal_browser_current(scope.clone())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(reloaded.view.reason.as_deref(), Some(NO_PROGRESS));
        assert_eq!(reloaded.view.revision, finished.revision);
        // Another member's scope can neither read nor guide this run.
        let (foreign, fowner, fuser, _fdir) = member("foreign").await;
        let (fscope, ..) = opened(&foreign, fowner, &fuser, "Foreign").await;
        let foreign_view = foreign
            .personal_browser_current(fscope)
            .await
            .unwrap()
            .unwrap()
            .view;
        assert_ne!(foreign_view.run_id, finished.run_id);
        assert!(foreign_view.reason.is_none());
        // A replayed guidance request does not change anything twice.
        let guide = Uuid::new_v4();
        let guided = store
            .personal_browser_guide(
                scope.clone(),
                finished.id.clone(),
                guide,
                "only mornings".into(),
            )
            .await
            .unwrap();
        assert_eq!(guided.view.reason.as_deref(), Some("new_guidance"));
        let again = store
            .personal_browser_guide(
                scope.clone(),
                finished.id.clone(),
                guide,
                "only mornings".into(),
            )
            .await
            .unwrap();
        assert_eq!(again.view.revision, guided.view.revision);
        // Taking control resolves a pause instead of leaving a stale reason on a live task.
        let (scope2, request2, mut view2) = opened(&store, owner, &user, "Second pause").await;
        view2.state = "ready".into();
        view2.reason = Some(NO_PROGRESS.into());
        let ready = store
            .personal_browser_finish(scope2.clone(), view2, request2, None)
            .await
            .unwrap();
        let Admission::Fresh(taken) = store
            .personal_browser_reserve(
                scope2.clone(),
                Version {
                    workspace_id: ready.id.clone(),
                    revision: ready.revision,
                    request_id: Uuid::new_v4(),
                },
                "takeover",
                Some("control-secret".into()),
            )
            .await
            .unwrap()
        else {
            panic!("expected a fresh takeover")
        };
        assert_eq!(taken.view.state, "user_control");
        assert!(taken.view.reason.is_none());
    }
}
