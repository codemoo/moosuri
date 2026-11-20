use super::*;
use crate::tests::{TEST_LOCK, admin, directory, member};
use moosuri_core::*;
async fn configured(s: &Store) -> UserContext {
    let u = admin(s).await.user;
    let p = s
        .create_provider(
            &u,
            ProviderChange {
                name: "Synthetic recall".into(),
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
    s.record_model_probe(&u, p.id.clone(), 1, "synthetic".into(), None)
        .await
        .unwrap();
    s.record_tool_probe(&u, p.id.clone(), 1, "synthetic".into(), None)
        .await
        .unwrap();
    s.set_default_model(
        &u,
        Some(ModelSelection {
            source: Default::default(),
            provider_id: p.id,
            provider_revision: 1,
            model: "synthetic".into(),
            effort: None,
        }),
    )
    .await
    .unwrap();
    u
}
async fn queued(s: &Store, u: &UserContext, t: &str, text: &str) -> ChatTurn {
    s.submit_turn(u, t.into(), Uuid::new_v4().to_string(), text.into())
        .await
        .unwrap()
}
async fn completed(s: &Store, u: &UserContext, t: &str, user: &str, answer: &str) -> ChatTurn {
    let turn = queued(s, u, t, user).await;
    let tenant = u.tenant.0.clone();
    let id = turn.run.id.clone();
    let text = answer.to_owned();
    s.call(move |db| {
        let c = &db.conn;
        c.execute(
            "UPDATE chat_runs SET state='completed',version=version+1 WHERE tenant_id=?1 AND id=?2",
            params![tenant, id],
        )?;
        c.execute(
            "INSERT INTO chat_output VALUES(?1,?2,'answer',?3,0,?4,'final_answer',0)",
            params![tenant, id, Uuid::new_v4().to_string(), text],
        )?;
        Ok(())
    })
    .await
    .unwrap();
    turn
}
#[tokio::test]
async fn history_is_bounded_owned_whole_role_labelled_and_revision_pinned() {
    let _lock = TEST_LOCK.lock().await;
    let dir = directory();
    let s = Store::open(dir.path()).unwrap();
    let u = configured(&s).await;
    let a = s.create_thread(&u, "history".into()).await.unwrap();
    let b = s.create_thread(&u, "other owned".into()).await.unwrap();
    let first = completed(
        &s,
        &u,
        &a.id,
        "needle original assertion",
        "needle assistant hypothesis",
    )
    .await;
    for i in 0..130 {
        completed(
            &s,
            &u,
            &a.id,
            &format!("padding {i}"),
            "unrelated synthetic text",
        )
        .await;
    }
    let elsewhere = completed(&s, &u, &b.id, "needle elsewhere", "other conversation").await;
    let failed = completed(&s, &u, &b.id, "needle failed", "must not disclose").await;
    let foreign = member(&s, &u, "foreign@example.test").await.user;
    let ft = s.create_thread(&foreign, "foreign".into()).await.unwrap();
    // An owned row in another account is a decoy even within the same installation.
    let tenant = u.tenant.0.clone();
    let failed_id = failed.run.id.clone();
    let foreign_id = foreign.account.id.clone();
    let ftid = ft.id;
    let fid = Uuid::new_v4().to_string();
    let foreign_tenant = foreign.tenant.0.clone();
    s.call(move|db|{db.conn.execute("UPDATE chat_runs SET state='failed' WHERE tenant_id=?1 AND id=?2",params![tenant,failed_id])?;db.conn.execute("INSERT INTO chat_runs SELECT ?6,?2,?3,?4,?5,payload_digest,1,'needle FOREIGN',model,session_digest,auth_epoch,1,'completed',1,NULL,created_at,admitted_order FROM chat_runs WHERE tenant_id=?1 LIMIT 1",params![tenant,fid,foreign_id,ftid,Uuid::new_v4().to_string(),foreign_tenant])?;db.conn.execute("INSERT INTO chat_output VALUES(?1,?2,'answer',?3,0,'needle FOREIGN answer','final_answer',0)",params![foreign_tenant,fid,Uuid::new_v4().to_string()])?;Ok(())}).await.unwrap();
    let current = queued(&s, &u, &a.id, "needle current request is not history").await;
    let tenant = u.tenant.0.clone();
    let rid = current.run.id.clone();
    let old = first.run.id.clone();
    let other = elsewhere.run.id.clone();
    s.call(move |db| {
        let c = &db.conn;
        let r = chat::load_run(c, &tenant, &rid)?;
        let local = search(c, &r, "needle", ConversationScope::CurrentThread)?;
        assert_eq!(local.hits.len(), 1);
        assert_eq!(local.hits[0].turn_id, old);
        let all = search(c, &r, "needle", ConversationScope::OwnedConversations)?;
        assert_eq!(all.hits.len(), 2);
        assert!(all.hits.iter().any(|v| v.turn_id == other));
        assert!(!all.truncated);
        let pin = local.hits[0].revision;
        let turn = read(c, &r, &old, pin)?;
        assert_eq!(turn.messages.len(), 2);
        assert!(matches!(turn.messages[0].role, HistoricalRole::User));
        assert!(matches!(turn.messages[1].role, HistoricalRole::Assistant));
        assert_eq!(turn.messages[0].text, "needle original assertion");
        assert_eq!(turn.created_at, local.hits[0].created_at);
        assert!(matches!(
            read(c, &r, &old, pin + 1),
            Err(StoreError::NotFound)
        ));
        c.execute(
            "UPDATE chat_runs SET version=version+1 WHERE tenant_id=?1 AND id=?2",
            params![tenant, old],
        )?;
        assert!(!self::super::current(c, &r)?);
        assert!(matches!(read(c, &r, &old, pin), Err(StoreError::Conflict)));
        let mut old_profile = r;
        old_profile
            .tool_profile
            .as_mut()
            .unwrap()
            .conversation_recall = false;
        assert!(matches!(
            search(c, &old_profile, "needle", ConversationScope::CurrentThread),
            Err(StoreError::InvalidInput)
        ));
        Ok(())
    })
    .await
    .unwrap();
}
#[tokio::test]
async fn every_search_candidate_and_context_descendant_is_purged_before_source_deletion() {
    let _lock = TEST_LOCK.lock().await;
    let dir = directory();
    let s = Store::open(dir.path()).unwrap();
    let u = configured(&s).await;
    let source = s.create_thread(&u, "source".into()).await.unwrap();
    let target = s.create_thread(&u, "target".into()).await.unwrap();
    completed(&s, &u, &source.id, "candidate one", "candidate one answer").await;
    completed(
        &s,
        &u,
        &source.id,
        "candidate two unread",
        "candidate two answer",
    )
    .await;
    let recalled = completed(&s, &u, &target.id, "question", "derived private answer").await;
    let descendant = completed(&s, &u, &target.id, "followup", "text-only descendant").await;
    let tenant = u.tenant.0.clone();
    let rid = recalled.run.id.clone();
    let did = descendant.run.id.clone();
    s.call(move|db|{
  let c=&db.conn;let r=chat::load_run(c,&tenant,&rid)?;let results=search(c,&r,"candidate",ConversationScope::OwnedConversations)?;assert_eq!(results.hits.len(),2);
  // Only one is read: deleting the other still purges the answer.
  read(c,&r,&results.hits[0].turn_id,results.hits[0].revision)?;
  crate::agent_execution::inherit_dependencies(c,&tenant,&did,&rid)?;
  let descendant=chat::load_run(c,&tenant,&did)?;assert!(matches!(read(c,&descendant,&results.hits[0].turn_id,results.hits[0].revision),Err(StoreError::NotFound)));
  for id in [&rid,&did]{c.execute("INSERT OR IGNORE INTO agent_execution(tenant_id,run_id,agent) VALUES(?1,?2,1)",params![tenant,id])?;c.execute("INSERT INTO agent_steps(tenant_id,run_id,step,attempt,nonce,state,checkpoint) VALUES(?1,?2,1,1,zeroblob(32),'completed',?3)",params![tenant,id,b"private checkpoint".as_slice()])?;}
  Ok(())
 }).await.unwrap();
    s.delete_thread(&u, source.id).await.unwrap();
    let page = s.conversation(&u, target.id.clone(), None).await.unwrap();
    assert!(
        page.turns
            .iter()
            .all(|t| t.assistant.is_empty() && !t.run.can_retry)
    );
    assert!(s.retry_run(&u, recalled.run.id, 1).await.is_err());
    let tenant = u.tenant.0.clone();
    s.call(move |db| {
        let n: i64 = db.conn.query_row(
            "SELECT count(*) FROM agent_steps WHERE tenant_id=?1 AND checkpoint IS NOT NULL",
            [tenant],
            |r| r.get(0),
        )?;
        assert_eq!(n, 0);
        Ok(())
    })
    .await
    .unwrap();
    drop(s);
    let reopened = Store::open(dir.path()).unwrap();
    assert!(
        reopened
            .conversation(&u, target.id, None)
            .await
            .unwrap()
            .turns
            .iter()
            .all(|t| t.assistant.is_empty())
    );
}
#[tokio::test]
async fn account_deletion_removes_recalled_output_dependencies_and_private_checkpoints() {
    let _lock = TEST_LOCK.lock().await;
    let dir = directory();
    let s = Store::open(dir.path()).unwrap();
    let administrator = configured(&s).await;
    let u = member(&s, &administrator, "delete-recall@example.test")
        .await
        .user;
    let source = s.create_thread(&u, "private source".into()).await.unwrap();
    let target = s.create_thread(&u, "private target".into()).await.unwrap();
    completed(
        &s,
        &u,
        &source.id,
        "private needle",
        "private earlier answer",
    )
    .await;
    let answer = completed(&s, &u, &target.id, "question", "private recalled answer").await;
    let tenant = u.tenant.0.clone();
    let rid = answer.run.id;
    let check_tenant = tenant.clone();
    s.call(move|db|{let c=&db.conn;let r=chat::load_run(c,&tenant,&rid)?;assert_eq!(search(c,&r,"needle",ConversationScope::OwnedConversations)?.hits.len(),1);c.execute("INSERT INTO agent_execution(tenant_id,run_id,agent) VALUES(?1,?2,1)",params![tenant,rid])?;c.execute("INSERT INTO agent_steps(tenant_id,run_id,step,attempt,nonce,state,checkpoint) VALUES(?1,?2,1,1,zeroblob(32),'completed',?3)",params![tenant,rid,b"private recalled checkpoint".as_slice()])?;Ok(())}).await.unwrap();
    let revision = s
        .managed_accounts(&administrator)
        .await
        .unwrap()
        .into_iter()
        .find(|a| a.user.id == u.account.id)
        .unwrap()
        .revision;
    let prepared = s
        .prepare_account_management(
            &administrator,
            u.account.id.clone(),
            AccountManagementInput {
                request_id: Uuid::new_v4().to_string(),
                expected_revision: revision,
                action: AccountManagementAction::Delete,
            },
        )
        .await
        .unwrap();
    s.apply_account_management(prepared).await.unwrap();
    s.call(move |db| {
        for table in [
            "chat_runs",
            "chat_output",
            "agent_steps",
            "agent_tool_slots",
            "conversation_recall_dependencies",
        ] {
            let n: i64 = db.conn.query_row(
                &format!("SELECT count(*) FROM {table} WHERE tenant_id=?1"),
                [&check_tenant],
                |r| r.get(0),
            )?;
            assert_eq!(n, 0, "{table}");
        }
        Ok(())
    })
    .await
    .unwrap();
    drop(s);
    let reopened = Store::open(dir.path()).unwrap();
    assert!(matches!(
        reopened.conversation(&u, target.id, None).await,
        Err(StoreError::Unauthorized)
    ));
}
#[tokio::test]
async fn bounded_candidates_skip_oversized_turns_without_adopting_undelivered_hits() {
    let _lock = TEST_LOCK.lock().await;
    let dir = directory();
    let s = Store::open(dir.path()).unwrap();
    let u = configured(&s).await;
    let t = s.create_thread(&u, "bounded history".into()).await.unwrap();
    for i in 0..7 {
        completed(
            &s,
            &u,
            &t.id,
            &format!("bounded-needle {i}"),
            "bounded-needle answer",
        )
        .await;
    }
    completed(
        &s,
        &u,
        &t.id,
        "bounded-needle oversized",
        &"x".repeat(WHOLE_BYTES + 1),
    )
    .await;
    let run = queued(&s, &u, &t.id, "current").await;
    let tenant = u.tenant.0.clone();
    let id = run.run.id;
    s.call(move|db|{let c=&db.conn;let r=chat::load_run(c,&tenant,&id)?;let page=search(c,&r,"bounded-needle",ConversationScope::CurrentThread)?;assert_eq!(page.hits.len(),5);assert!(page.truncated&&!page.scanned_limit);assert!(page.hits.iter().all(|h|!h.user_excerpt.contains("oversized")));let count:i64=c.query_row("SELECT count(*) FROM conversation_recall_dependencies WHERE tenant_id=?1 AND run_id=?2",params![tenant,id],|v|v.get(0))?;assert_eq!(count,5);assert!(serde_json::to_vec(&crate::KnowledgeToolResult::ConversationSearch(page)).unwrap().len()<crate::agent_execution::TOOL_RESULT_LIMIT);assert!(matches!(search(c,&r,"\n",ConversationScope::CurrentThread),Err(StoreError::InvalidInput)));Ok(())}).await.unwrap();
}

#[tokio::test]
async fn display_only_preview_stays_in_owner_history_but_not_model_context_or_recall_after_reopen()
{
    // Unconditional test: this boundary also runs with credential feature OFF.
    let _lock = TEST_LOCK.lock().await;
    let dir = directory();
    let s = Store::open(dir.path()).unwrap();
    let u = configured(&s).await;
    let thread = s
        .create_thread(&u, "device-only history".into())
        .await
        .unwrap();
    let old = completed(
        &s,
        &u,
        &thread.id,
        "내 Gmail 받은편지함 미리보기 보여줘",
        "PRIVATE_PREVIEW_DISPLAY_ONLY_19 old mail",
    )
    .await;
    let newer = completed(
        &s,
        &u,
        &thread.id,
        "Please show my Gmail inbox preview",
        "PRIVATE_PREVIEW_DISPLAY_ONLY_19 new mail",
    )
    .await;
    completed(
        &s,
        &u,
        &thread.id,
        "Gmail counts request",
        "Counts remain model-visible: 21",
    )
    .await;
    let tenant = u.tenant.0.clone();
    let account = u.account.id.clone();
    let epoch = u.epoch;
    let old_id = old.run.id.clone();
    let new_id = newer.run.id.clone();
    s.call(move|db|{
        let c=&db.conn;let request=Uuid::new_v4().to_string();let resource=r#"{"google_inbox":"preview_v1"}"#;
        c.execute("UPDATE chat_output SET item='credential-google-result' WHERE tenant_id=?1 AND run_id=?2",params![tenant,old_id])?;
        c.execute("INSERT INTO credential_broker_jobs(tenant_id,account_id,request_id,run_id,call_id,invocation_nonce,repository,auth_epoch,expires_at,request) VALUES(?1,?2,?3,?4,'call','nonce',?5,?6,unixepoch()+300,NULL)",params![tenant,account,request,old_id,resource,epoch])?;
        c.execute("INSERT INTO credential_chat_waits VALUES(?1,?2,1,1,'call','call','nonce',?3,?4,'settled',unixepoch()+300)",params![tenant,old_id,request,resource])?;
        c.execute("UPDATE chat_output SET item='credential-google-preview-result' WHERE tenant_id=?1 AND run_id=?2",params![tenant,new_id])?;
        Ok(())
    }).await.unwrap();
    drop(s);
    let s = Store::open(dir.path()).unwrap();
    assert!(
        serde_json::to_string(&s.conversation(&u, thread.id.clone(), None).await.unwrap())
            .unwrap()
            .contains("PRIVATE_PREVIEW_DISPLAY_ONLY_19")
    );
    let current = queued(&s, &u, &thread.id, "current clean request").await;
    let tenant = u.tenant.0.clone();
    let current_id = current.run.id;
    let old_id = old.run.id;
    s.call(move |db| {
        let c = &db.conn;
        let mut run = chat::load_run(c, &tenant, &current_id)?;
        let profile = run.tool_profile.as_mut().unwrap();
        profile.credential_google = false;
        profile.credential_google_preview = false;
        let context = chat::context(c, &run)?;
        let text = context
            .iter()
            .map(|m| m.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(!text.contains("PRIVATE_PREVIEW_DISPLAY_ONLY_19"));
        assert!(text.contains("Counts remain model-visible"));
        assert!(
            search(
                c,
                &run,
                "PRIVATE_PREVIEW_DISPLAY_ONLY_19",
                ConversationScope::OwnedConversations
            )?
            .hits
            .is_empty()
        );
        let page = search(c, &run, "Gmail", ConversationScope::OwnedConversations)?;
        assert_eq!(page.hits.len(), 1);
        let source = chat::load_run(c, &tenant, &old_id)?;
        assert!(matches!(whole(c, &source), Err(StoreError::NotFound)));
        // Even a previously retained discovery pin cannot disclose this content.
        c.execute(
            "INSERT INTO conversation_recall_dependencies VALUES(?1,?2,?3,?4,1)",
            params![tenant, current_id, old_id, source.version],
        )?;
        assert!(matches!(
            read(c, &run, &old_id, source.version),
            Err(StoreError::NotFound)
        ));
        Ok(())
    })
    .await
    .unwrap();
}

mod memory_reader;
