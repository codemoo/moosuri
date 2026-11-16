//! Bounded CURRENT-user commands. Quoted mail/model text cannot register a watch.
use super::*;
use moosuri_core::{MessagePhase, RunState};
#[derive(PartialEq, Debug)]
enum Command {
    Start(u32),
    Stop,
    Status,
    Suggest,
}
fn command(text: &str) -> Option<Command> {
    // Whole utterances, not substring matching across questions, quotes or negation.
    let text = text
        .trim()
        .trim_end_matches(['.', '!', '。'])
        .to_lowercase();
    let text = text.trim();
    let (text, cadence) = [("5분마다 ", 300), ("30분마다 ", 1800), ("1시간마다 ", 3600)]
        .iter()
        .find_map(|(prefix, seconds)| text.strip_prefix(prefix).map(|rest| (rest, *seconds)))
        .unwrap_or((text, 1800));
    if text.ends_with("메일 기다려보지뭐") && !text.contains(['"', '\'', '?', '\n']) {
        return Some(Command::Suggest);
    }
    match text {
        "답장 오면 알려줘"
        | "회신 오면 알려줘"
        | "메일 답장 오면 알려줘"
        | "새 답장 오면 알려줘"
        | "이 메일 답장 오면 알려줘"
        | "메일 회신 오면 알려줘"
        | "답장 오면 알려주세요"
        | "메일 답장 감시해줘"
        | "notify me when they reply"
        | "watch for a reply" => Some(Command::Start(cadence)),
        "메일 감시 중지"
        | "메일 감시 중지해"
        | "메일 감시 그만해"
        | "답장 감시 중지"
        | "답장 감시 중지해줘"
        | "stop watching for a reply" => Some(Command::Stop),
        "메일 감시 상태"
        | "답장 감시 상태"
        | "메일 감시 중이야?"
        | "답장 확인 중이야?"
        | "reply watch status" => Some(Command::Status),
        "메일 기다려보자" | "메일 기다려보지 뭐" | "메일 기다려보지뭐" | "그럼 메일 기다려보자" => {
            Some(Command::Suggest)
        }
        _ => None,
    }
}
fn confirmation(c: &Connection, run: &crate::chat::Run, text: String) -> Result<bool> {
    crate::chat::apply_patches(
        c,
        run,
        vec![crate::OutputPatch::Append {
            item: "mail-reply-watch".into(),
            text,
            phase: Some(MessagePhase::FinalAnswer),
            refusal: false,
        }],
    )?;
    crate::chat::transition(c, run, RunState::Completed, None)?;
    Ok(true)
}
pub(crate) fn finish_chat(
    c: &Connection,
    user: &UserContext,
    run: &crate::chat::Run,
) -> Result<bool> {
    let Some(command) = command(&run.text) else {
        return Ok(false);
    };
    if crate::media::has_context(c, &run.tenant, &run.id)? {
        return Ok(false);
    }
    sweep(c, Some(&run.tenant))?;
    let ids=c.prepare("SELECT id FROM mail_reply_watches WHERE tenant_id=?1 AND account_id=?2 AND thread_id=?3 AND state='active' ORDER BY created_at DESC")?.query_map(params![run.tenant,run.account,run.thread],|r|r.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
    let text=match command {
        Command::Status => if ids.is_empty(){"이 대화에서 진행 중인 메일 답장 감시는 없어요. 아래 카드에서 지난 결과나 중지 사유를 확인할 수 있어요.".into()}else{format!("이 대화에 등록된 답장 감시 {}건이 있어요. 아래 카드에서 최근 확인 시각과 다음 조회, 오류 여부를 확인할 수 있어요. 새 답장이 없으면 모델을 호출하지 않아요.",ids.len())},
        Command::Stop => if ids.len()==1 {withdraw(c,&run.tenant,&ids[0],"stopped","owner_stopped")?;"이 대화의 메일 답장 감시를 중지했어요.".into()}else if ids.is_empty(){"이 대화에서 진행 중인 답장 감시가 없어요.".into()}else{"감시 중인 메일이 여러 개예요. 아래 카드에서 중지할 메일을 선택해 주세요.".into()},
        Command::Suggest if ids.is_empty() => "답장이 오면 알려드릴 수 있어요. 아래 메일 카드에서 ‘답장 기다리기’를 누르거나 ‘답장 오면 알려줘’라고 말씀해 주세요. 아직 새 감시를 등록하지 않았어요.".into(),
        Command::Suggest => "이 대화에 등록된 메일 답장 감시는 계속 진행돼요. 최근 확인 결과는 아래 카드에서 볼 수 있어요.".into(),
        Command::Start(cadence) => {
            let targets=targets::list(c,user,&run.thread)?;
            if targets.len()!=1 { if targets.is_empty(){"감시할 메일을 먼저 열어 주세요. 이 대화에서 실제로 읽은 메일이나 발송이 확인된 메일에 답장 감시를 연결할 수 있어요. 아직 감시를 등록하지 않았어요.".into()}else{"어떤 메일의 답장을 기다릴까요? 아래 카드에서 대상을 선택해 주세요. 아직 새 감시를 등록하지 않았어요.".into()} }
            else {
                let watch=create(c,user,&MailReplyWatchCreate{request_id:run.request.clone(),thread_id:run.thread.clone(),target_id:targets[0].view.id.clone(),cadence_seconds:cadence,duration_days:7})?;
                format!("이 메일의 답장 감시가 등록되어 있어요. {}분마다 확인하고, 새 답장이 오면 본문 미리보기와 소식함으로 알려드릴게요. 첫 답장을 찾거나 아래 카드의 종료 시각이 되면 종료해요. 새 메일이 없을 때는 모델 호출·토큰 사용이 없고 Gmail API 조회만 발생해요. 아래 카드에서 상태 확인과 중지가 가능해요.",watch.cadence_seconds/60)
            }
        }
    };
    confirmation(c, run, text)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_whole_current_utterances_authorize_followup() {
        assert_eq!(command("답장 오면 알려줘!"), Some(Command::Start(1800)));
        for text in [
            "답장 오면 알려줘라고 말했었어",
            "\"답장 오면 알려줘\"",
            "답장 오면 알려주지 마",
            "답장 오면 알려줄 수 있어?",
            "메일 내용: 답장 오면 알려줘",
            "배송조회 답장 오면 알려줘",
        ] {
            assert_eq!(command(text), None);
        }
        assert_eq!(command("메일 기다려보지뭐"), Some(Command::Suggest));
    }
}
