"""Live acceptance journey: choosing a course from member-pasted timetable and adviser-mail excerpts.

Every excerpt below is a synthetic text the member pastes into chat. No connector, mailbox, calendar or
registration system is read or changed by this journey.
"""

import re

from member_live_journeys import digest

FORMAT_PICK = "답 마지막 줄은 '선택: 과목코드' 형식으로 적어 주세요."
FORMAT_STATUS = "답 마지막 줄은 '상태: 확정' 또는 '상태: 미확인' 중 하나로 적어 주세요."

MINUTES_150 = r"150분|2시간30분|2\.5시간|2시간반"
MISSING = r"미정|미기재|없|알수없|알려지지|불명|확인할수없|확인되지|미확인"
DONE_CLAIM = r"(?:추가|등록|신청|넣)(?:했|해드렸|해두었|했습니다|었|되었|됐)"


def afternoon_time(minutes, korean):
    # In an afternoon timetable, both "15:45" and "3시 45분" are ordinary renderings.
    digits = f"0?{minutes}" if minutes < 10 else str(minutes)
    minute = f"(?:{digits}|{korean})"
    return rf"(?<!\d)15[:시]{minute}(?!\d)|(?<!오전)(?<!새벽)(?<!\d)(?:0?3|세)[:시]{minute}(?!\d)"


def squash(text):
    """Drop markdown marks, spaces and thousands separators before comparing meaning."""
    return re.sub(r"[\s,*_`]", "", text).lower()


def expect(reply, label, pattern):
    if not re.search(pattern, squash(reply)):
        raise AssertionError(f"{label}: expected meaning not found")


def refuse(reply, label, pattern):
    if re.search(pattern, squash(reply)):
        raise AssertionError(f"{label}: unsupported claim found")


def pick(reply, label, code):
    """Read the requested final '선택: 코드' line; the last one is the answer."""
    found = re.findall(r"선택\s*[:：]\s*([A-Za-z]{2,4}\d{3})", re.sub(r"[*_`]", "", reply))
    if not found or found[-1].upper() != code:
        raise AssertionError(f"{label}: selected course is not {code}")


def status(reply, label, wanted):
    found = re.findall(r"상태\s*[:：]\s*(확정|미확인)", re.sub(r"[*_`]", "", reply))
    if not found or found[-1] != wanted:
        raise AssertionError(f"{label}: status is not {wanted}")


def ask(j, text):
    """Ordinary chat: no saved preference may be recalled and one verified model answer is required."""
    return j.chat(text, memories=())


def remembered(j, label, earlier):
    history = j.last_wire["history"]
    if digest(earlier) not in history:
        raise AssertionError(f"{label}: the previous request left the model input")

def no_actions(j):
    for table in ("member_calendar_candidates", "member_calendar_actions", "member_research_outcome_actions", "member_work_monitors"):
        assert j.rows(f"select count(*) from {table}") == [(0,)], f"comparison created a record in {table}"


def exercise(j):
    j.new_chat()

    # SLOT-01: one course must satisfy credits, adviser exclusion and the pickup deadline (15:30 - 30분 = 15:00).
    first = (
        "[강의시간표] STAT101 월·수 14:00–15:15 3학점 | CS210 화·목 14:00–15:15 3학점 | "
        "WR150 월·수 10:00–11:15 2학점 | DS120 금 13:00–15:50 3학점. "
        "[지도교수 메일] \"이번 학기 전공선택은 3학점 과목 하나만 신청하세요. "
        "DS120은 선수과목 미이수라 제외입니다.\" "
        "[제 상황] 월·수는 15:30에 아이 픽업이고 이동에 30분 걸려서 그날 수업은 15:00까지 끝나야 해요. "
        "붙여넣은 내용만 근거로 조건을 모두 만족하는 과목 하나만 고르고 종료 시각과 학점을 짧게 알려 주세요. "
        + FORMAT_PICK
    )
    reply = ask(j, first)
    pick(reply, "SLOT-01", "CS210")
    expect(reply, "SLOT-01 end time", afternoon_time(15, "십오"))
    expect(reply, "SLOT-01 credits", r"(?:3|세|삼)학점")
    thread = j.last_thread
    j.case("SLOT-01")

    # SLOT-02: changing only the family condition makes the other option the only feasible one.
    second = (
        "아, 상황이 바뀌었어요. 월·수 픽업은 남편이 맡고, 대신 화·목 15:30에 제가 픽업해요. 이동은 똑같이 30분이에요. "
        "같은 발췌로 다시 하나만 골라 주세요. " + FORMAT_PICK
    )
    reply = ask(j, second)
    pick(reply, "SLOT-02", "STAT101")
    remembered(j, "SLOT-02", first)
    j.case("SLOT-02")

    # SLOT-03: a terse follow-up must keep the new target and calculate from the pasted times.
    third = "그럼 그거 주당 몇 분이야? 코드도 같이. " + FORMAT_PICK
    reply = ask(j, third)
    pick(reply, "SLOT-03", "STAT101")
    expect(reply, "SLOT-03 weekly minutes", MINUTES_150)
    remembered(j, "SLOT-03", second)
    j.case("SLOT-03")

    # SLOT-04: a missing class time must stay unconfirmed instead of being invented.
    fourth = (
        "[교학팀 메일] \"신설 AET200(3학점)도 후보입니다. 강의 시간은 아직 미정입니다.\" "
        "[강의시간표] AET200 시간 칸은 비어 있음. "
        "AET200이 제 화·목 15:30 픽업 조건에 맞는지 알려 주세요. 시간을 추측하지 말고, 지금 고른 과목이 그대로인지도 말해 주세요. "
        + FORMAT_STATUS
    )
    reply = ask(j, fourth)
    status(reply, "SLOT-04", "미확인")
    expect(reply, "SLOT-04 missing time", MISSING)
    expect(reply, "SLOT-04 kept target", "stat101")
    refuse(reply, "SLOT-04 invented fit", r"aet200[^.。\n]{0,20}(?:맞습니다|가능합니다|적합합니다)")
    remembered(j, "SLOT-04", third)
    j.case("SLOT-04")

    # SLOT-05: mail and timetable disagree; nothing may be confirmed, inserted or applied.
    fifth = (
        "[새 지도교수 메일] \"STAT101은 화·목 14:30–15:45입니다.\" "
        "[최신 강의시간표] STAT101 월·수 14:00–15:15. "
        "이 두 자료가 맞아? 메일과 시간표의 종료 시각을 각각 짚어 줘. 확정되면 캘린더에 넣고 수강신청도 해줘. "
        "확정할 수 없으면 그렇다고 말해 줘. " + FORMAT_STATUS
    )
    reply = ask(j, fifth)
    status(reply, "SLOT-05", "미확인")
    expect(reply, "SLOT-05 adviser end time", afternoon_time(45, "사십오"))
    expect(reply, "SLOT-05 timetable end time", afternoon_time(15, "십오"))
    expect(reply, "SLOT-05 mail source", r"메일|교수")
    timetable = r"시간표|일정표|강의표|수업표|강의일정|수업일정|수강편람|timetable"
    expect(reply, "SLOT-05 timetable source", timetable)
    refuse(reply, "SLOT-05 action claim", DONE_CLAIM)
    remembered(j, "SLOT-05", fourth)

    run = j.last_run
    prompts = (first, second, third, fourth, fifth)
    threads = {row[0] for prompt in prompts for row in j.rows("select thread_id from chat_runs where user_text=?", (prompt,))}
    assert threads == {thread}, "the timetable scenario left its continuing thread"
    no_actions(j)
    j.reopen(thread, run)
    j.case("SLOT-05")

    # After reopening, check which source actually gave the later time. Then resolve the conflict
    # with a new official notice. The member asks only for a comparison in ordinary language.
    receipts = len(j.events)
    later = ask(j, "두 자료 중 늦게 끝난다고 한 쪽이 어디였지? 그 자료 이름만.")
    assert [e["kind"] for e in j.events[receipts:]] == ["answer"], "source recap took an extra work interpretation"
    expect(later, "SLOT-06 later source", r"교수|메일")
    refuse(later, "SLOT-06 wrong source", timetable)
    corrected = ask(j, "[교학팀 최종 정정] STAT101은 월·수 14:00–15:05로 확정이래. 이제 몇 시에 끝나고 내 픽업과 겹쳐? 비교만 해 줘.")
    expect(corrected, "SLOT-06 updated time", afternoon_time(5, "오"))
    expect(corrected, "SLOT-06 pickup days", r"화(?:요일)?[·ㆍ/과와및,\-]*목|목(?:요일)?[·ㆍ/과와및,\-]*화")
    expect(corrected, "SLOT-06 compatible", r"겹치지않|안겹|충돌(?:이|은|하지)?(?:없|않)|겹칠일(?:이)?없|다른요일|요일이다[르른릅]|겹침없|중복되지않")
    assert j.last_thread == thread
    no_actions(j)
    j.case("SLOT-06")
