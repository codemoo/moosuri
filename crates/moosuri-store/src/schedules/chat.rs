//! Direct CURRENT-user fixed reminders. No model/context/tool can call this admission body.
use super::*;
use crate::chat::{self as conversation, Run};
use moosuri_core::{MessagePhase, RunState};
mod drafts;
#[cfg(test)]
mod integration_tests;
pub(crate) mod parser;
#[cfg(test)]
mod tests;
pub(crate) fn validate_timezone(value: &str) -> Result<()> {
    time::zone(value).map(|_| ())
}
#[allow(dead_code)]
const MISSION: &str = "현재 예약은 정해진 문구를 개인 소식함에 남기는 알림만 지원합니다. 조사·검색·메일 발송 같은 예약 작업은 아직 실행하지 않습니다. 알림 문구를 지정해 주세요.";
fn target(
    conn: &Connection,
    user: &UserContext,
    run: &Run,
    hint: &parser::Target,
    selected: Option<&ScheduleTarget>,
) -> Result<Option<Schedule>> {
    if let Some(t) = selected {
        let s = owned(conn, user, &t.id)?;
        if s.view.revision != t.revision || s.view.state == "revoked" {
            return Err(StoreError::Conflict);
        }
        return Ok(match hint {
            parser::Target::Generic | parser::Target::Previous => Some(s),
            parser::Target::Title(title) if title == &s.view.title => Some(s),
            _ => None,
        });
    }
    if matches!(hint, parser::Target::Previous) {
        // A deictic control names the original immediately preceding receipt,
        // never a global most-recent or sole reminder. Identity/revision is data;
        // the explicit current control and owned() remain the action authority.
        let prior: Option<String> = conn.query_row(
            "SELECT c.receipt FROM chat_runs p JOIN schedule_chat_commands c ON c.tenant_id=p.tenant_id AND c.run_id=p.id WHERE p.tenant_id=?1 AND p.account_id=?2 AND p.thread_id=?3 AND p.sequence=?4 AND p.state='completed' AND p.session_digest=?5 AND p.auth_epoch=?6 AND p.created_at>=?7",
            params![run.tenant,run.account,run.thread,run.sequence-1,run.session.as_slice(),run.epoch,clock()?-1800],
            |r| r.get(0),
        ).optional()?;
        let Some(prior) = prior else { return Ok(None) };
        let receipt: ScheduleChatReceipt = decode(&prior)?;
        let s = match owned(conn, user, &receipt.id) {
            Ok(s) => s,
            Err(StoreError::NotFound | StoreError::Conflict) => return Ok(None),
            Err(e) => return Err(e),
        };
        return Ok((s.view.revision == receipt.revision && s.view.state != "revoked").then_some(s));
    }
    let ids=conn.prepare("SELECT id FROM reminder_schedules WHERE tenant_id=?1 AND account_id=?2 AND state<>'revoked' AND NOT EXISTS(SELECT 1 FROM followup_reminders f WHERE f.tenant_id=reminder_schedules.tenant_id AND f.schedule_id=reminder_schedules.id) AND NOT EXISTS(SELECT 1 FROM proactive_suggestions p WHERE p.tenant_id=reminder_schedules.tenant_id AND p.schedule_id=reminder_schedules.id) AND NOT EXISTS(SELECT 1 FROM proactive_topics p WHERE p.tenant_id=reminder_schedules.tenant_id AND p.schedule_id=reminder_schedules.id) AND NOT EXISTS(SELECT 1 FROM recurring_missions x WHERE x.tenant_id=reminder_schedules.tenant_id AND x.schedule_id=reminder_schedules.id) ORDER BY id LIMIT 65")?.query_map(params![user.tenant.0,user.account.id],|r|r.get::<_,String>(0))?.collect::<std::result::Result<Vec<_>,_>>()?;
    if ids.len() > 64 {
        return Err(StoreError::Conflict);
    }
    let all = ids
        .into_iter()
        .map(|id| load(conn, &user.tenant.0, &id))
        .collect::<Result<Vec<_>>>()?;
    let count = all.len();
    let mut matching = all
        .into_iter()
        .filter(|s| match hint {
            parser::Target::Generic => count == 1,
            parser::Target::Title(title) => title == &s.view.title,
            parser::Target::Previous => false,
        })
        .collect::<Vec<_>>();
    Ok(if matching.len() == 1 {
        matching.pop()
    } else {
        None
    })
}
pub(crate) fn receipts(
    conn: &Connection,
    tenant: &str,
    run: &str,
) -> Result<Vec<ScheduleChatReceipt>> {
    let row: Option<String> = conn
        .query_row(
            "SELECT receipt FROM schedule_chat_commands WHERE tenant_id=?1 AND run_id=?2",
            params![tenant, run],
            |r| r.get(0),
        )
        .optional()?;
    row.map(|r| decode(&r))
        .transpose()
        .map(|r| r.into_iter().collect())
}
pub(crate) fn finish(
    conn: &Connection,
    user: &UserContext,
    run: &Run,
    context: Option<&ScheduleChatContext>,
    independent_target: bool,
) -> Result<bool> {
    let selected = context.and_then(|c| c.target.as_ref());
    let now = clock()?;
    let (decision, timezone) = drafts::decide(conn, run, context, now, independent_target)?;
    let timezone = timezone.as_deref();
    let (message, receipt) = match decision {
        parser::Decision::None => return Ok(false),
        parser::Decision::Clarify(text) => (text.to_owned(), None),
        parser::Decision::Mission if run.tool_profile.is_some() => return Ok(false),
        parser::Decision::Mission => (MISSION.into(), None),
        parser::Decision::Command(command) => {
            if crate::media::has_context(conn, &run.tenant, &run.id)? {
                confirmation(conn, run, "첨부 자료가 연결된 대화에서는 알림을 변경하지 않습니다. 새 대화에서 알림을 요청하거나 예약 화면을 이용해 주세요.".into(), None)?;
                return Ok(true);
            }
            use parser::Action;
            let action = match command.action {
                Action::Create => "create",
                Action::Edit => "edit",
                Action::Pause => "pause",
                Action::Resume => "resume",
                Action::Revoke => "revoke",
            };
            let current = if command.action == Action::Create
                || independent_target
                    && matches!(command.target.as_ref(), Some(parser::Target::Previous))
            {
                None
            } else {
                target(
                    conn,
                    user,
                    run,
                    command.target.as_ref().ok_or(StoreError::Corrupt)?,
                    selected,
                )?
            };
            let output = if command.action == Action::Create {
                let message = command.message.ok_or(StoreError::Corrupt)?;
                let mut title = String::new();
                for c in message.chars() {
                    if title.len() + c.len_utf8() > 160 {
                        break;
                    }
                    title.push(c)
                }
                create_in(
                    conn,
                    user,
                    &ScheduleCreate {
                        request_id: run.request.clone(),
                        draft: ScheduleDraft {
                            title,
                            message,
                            rule: command.rule.ok_or(StoreError::Corrupt)?,
                            valid_until: None,
                        },
                    },
                    now,
                )?
            } else if let Some(s) = current {
                if command.action == Action::Edit {
                    let rule = if let Some(seconds) = command.shift_seconds {
                        let ScheduleRule::Once { at } = s.view.rule else {
                            return confirmation(conn, run, "반복 알림은 바꿀 시각이나 반복 주기를 정확히 알려주세요. 이번 회차만 미룰지 전체 반복 시각을 바꿀지 임의로 정하지 않습니다.".into(), None).map(|()| true);
                        };
                        let shifted = at.checked_add(seconds).filter(|at| {
                            *at > now
                                && time::instant(*at).is_ok()
                                && s.view.valid_until.is_none_or(|until| *at <= until)
                        });
                        let Some(at) = shifted else {
                            return confirmation(conn, run, "바꿀 시각이 이미 지났거나 알림의 유효 기간을 벗어납니다. 앞으로 알려드릴 날짜와 시각을 지정해 주세요.".into(), None).map(|()| true);
                        };
                        ScheduleRule::Once { at }
                    } else {
                        command.rule.unwrap_or(s.view.rule)
                    };
                    edit_in(
                        conn,
                        user,
                        &s.view.id,
                        &ScheduleEdit {
                            expected_revision: s.view.revision,
                            draft: ScheduleDraft {
                                title: s.view.title,
                                message: command.message.unwrap_or(s.view.message),
                                rule,
                                valid_until: s.view.valid_until,
                            },
                        },
                        now,
                    )?
                } else {
                    control_in(
                        conn,
                        user,
                        &s.view.id,
                        &ScheduleControl {
                            expected_revision: s.view.revision,
                            action: action.into(),
                        },
                        now,
                    )?
                }
            } else {
                return confirmation(conn,run,"어떤 알림을 바꿀까요? 알림의 정확한 이름을 포함하거나, 저장 영수증에서 알림을 선택한 뒤 다시 요청해 주세요.".into(),None).map(|()|true);
            };
            let receipt = ScheduleChatReceipt {
                request_id: run.request.clone(),
                id: output.id.clone(),
                revision: output.revision,
                action: action.into(),
                state: output.state.clone(),
                title: output.title.clone(),
                next_local_slot: output.next_local_slot.clone(),
                timezone: match &output.rule {
                    ScheduleRule::Daily { timezone, .. }
                    | ScheduleRule::Weekly { timezone, .. } => timezone.clone(),
                    _ => timezone.unwrap_or("UTC").into(),
                },
                url: "/app/schedules".into(),
            };
            let status = match action {
                "pause" => "알림을 일시 정지했습니다",
                "resume" if output.state == "expired" => {
                    "지난 회차가 끝났습니다. 새 시각으로 편집해 주세요"
                }
                "resume" => "알림을 다시 시작했습니다",
                "revoke" => "알림을 삭제했습니다",
                "edit" => "알림을 변경했습니다",
                _ => "개인 알림을 저장했습니다",
            };
            let mut message = format!("{status}: {}", output.title);
            if output.state == "active" {
                if let Some(at) = output.next_fire_at {
                    let date =
                        chrono::DateTime::from_timestamp(at, 0).ok_or(StoreError::Corrupt)?;
                    let zone = time::zone(&receipt.timezone)?;
                    message.push_str(&format!(
                        "\n다음 알림: {}",
                        date.with_timezone(&zone)
                            .format("%Y년 %-m월 %-d일 %H:%M (%Z)")
                    ));
                }
                message.push_str("\n예약한 일정에 따라 소식함으로 알려드릴게요.");
            }
            (message, Some(receipt))
        }
    };
    confirmation(conn, run, message, receipt)?;
    Ok(true)
}
fn confirmation(
    conn: &Connection,
    run: &Run,
    message: String,
    receipt: Option<ScheduleChatReceipt>,
) -> Result<()> {
    if let Some(receipt) = receipt {
        conn.execute(
            "INSERT INTO schedule_chat_commands VALUES(?1,?2,1,?3)",
            params![run.tenant, run.id, encode(&receipt)?],
        )?;
    }
    conversation::apply_patches(
        conn,
        run,
        vec![crate::OutputPatch::Append {
            item: "schedule-confirmation".into(),
            text: message,
            phase: Some(MessagePhase::FinalAnswer),
            refusal: false,
        }],
    )?;
    conversation::transition(conn, run, RunState::Completed, None)
}
