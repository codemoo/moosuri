"""Live acceptance journey: a connected Korean business trip planning chat."""

import re

from member_live_journeys import digest

PREFERENCE = "저는 숙소는 조용한 곳을 선호해요"

TIME_3PM = r"오후3시|(?<!\d)15(?:시|:00)|(?<!\d)3pm"
TIME_1630 = r"오후4(?:시(?:30분|반)|:30)|(?<!\d)16(?:시30분|:30)|(?<!오전)(?<!\d)4(?:시(?:30분|반)|:30)"


def ask(j, text):
    """Ask an ordinary trip question that must not recall the lodging preference."""
    return j.chat(text, memories=())


def squash(text):
    """Drop spaces and thousands separators so '48만 원' and '480,000' compare easily."""
    return re.sub(r"[\s,]", "", text).lower()


def expect(reply, label, pattern):
    if not re.search(pattern, squash(reply)):
        raise AssertionError(f"{label}: expected answer not found")


def refuse(reply, label, pattern):
    if re.search(pattern, squash(reply)):
        raise AssertionError(f"{label}: unexpected answer found")


def exercise(j):
    j.new_chat()
    j.remember(PREFERENCE)

    # PLAN-01: establish a day/time, then correct only the day.
    first_prompt = "부산 고객 미팅이 이번 주 화요일 오후 3시예요. 요일과 시간을 짧게 정리해 주세요."
    first = ask(j, first_prompt)
    expect(first, "PLAN-01 initial day", "화요일")
    expect(first, "PLAN-01 initial time", TIME_3PM)
    trip_thread, trip_run = j.last_thread, j.last_run

    corrected = ask(j, "아, 요일만 수요일로 바뀌었어요. 최종 요일과 시간을 다시 알려 주세요.")
    expect(corrected, "PLAN-01 corrected day", "수요일")
    expect(corrected, "PLAN-01 kept time", TIME_3PM)
    j.case("PLAN-01")

    # PLAN-02: eight different follow-ups with independently checkable answers.
    steps = [
        ("미팅 장소는 부산역 바로 앞이고 30분 전에 도착하고 싶어요. 서울에서 KTX가 2시간 30분 걸리면 늦어도 몇 시에 출발해야 하나요?",
         "departure time", r"정오|낮12|(?<!오전)(?<!\d)12(?:시|:00)"),
        ("참석자는 저 포함 4명이고 KTX 왕복이 1인 12만원이에요. 교통비 합계는 얼마인가요?",
         "four-person fare", r"(?<!\d)(?:480000(?!\d)|48만원)"),
        ("한 명이 빠지면 KTX 합계는 얼마가 되나요?",
         "three-person fare", r"(?<!\d)(?:360000(?!\d)|36만원)"),
        ("부산 시내 택시를 한 대로 왕복하면 총 3만원이에요. 방금 KTX 합계에 더하면 총 교통비는요?",
         "transport total", r"(?<!\d)(?:390000(?!\d)|39만원)"),
        ("미팅이 1시간 30분 걸리면 몇 시에 끝나나요?",
         "meeting end", TIME_1630),
        ("끝나고 정확히 30분 뒤에 부산역에서 서울행 KTX를 탄다면 출발 시각은요?",
         "return departure", r"(?<!\d)17(?:시|:00)|(?<!오전)(?<!\d)5(?:시|:00)"),
        ("그 열차가 2시간 30분 걸리면 서울 도착 시각은 언제예요?",
         "arrival time", r"(?<!\d)19(?:시30분|:30)|(?<!오전)(?<!\d)7(?:시(?:30분|반)|:30)"),
        ("교통비는 3명 기준 39만원이었죠. 식비를 1인 2만 5천원씩 3명으로 잡으면 합쳐서 얼마인가요?",
         "combined budget", r"(?<!\d)(?:465000(?!\d)|46만(?:5천|5000)원|46\.5만원)"),
    ]
    for prompt, label, pattern in steps:
        expect(ask(j, prompt), f"PLAN-02 {label}", pattern)
    j.case("PLAN-02")

    # PLAN-03: the saved preference should decide a quiet vs noisy choice.
    lodging_prompt = (
        "가격과 거리가 같은 숙소 두 곳 중 하나만 골라 주세요. "
        "해송 스테이는 밤에도 아주 조용한 주택가에 있고, "
        "광안 스테이는 밤새 클럽 음악이 들리는 번화가에 있어요. 추천하는 곳 이름만 알려 주세요."
    )
    choice = j.chat(lodging_prompt, memories=(PREFERENCE,))
    expect(choice, "PLAN-03 quiet choice", "해송")
    refuse(choice, "PLAN-03 noisy choice", "광안")

    history = j.last_wire["history"]
    if digest(first_prompt) in history:
        raise AssertionError("PLAN-03: first prompt should be outside recent history")
    if not (2 <= len(history) <= 16 and len(history) % 2 == 0):
        raise AssertionError("PLAN-03: expected 1-8 retained message pairs")
    if digest(steps[-1][0]) not in history:
        raise AssertionError("PLAN-03: previous prompt should remain in recent history")
    j.case("PLAN-03")

    # Use another conversation before returning to the original stored answer.
    j.new_chat()
    second = j.chat("상자 하나에 수건 세 장씩, 일곱 상자면 수건이 몇 장이야? 숫자만 답해줘.", memories=())
    assert second.strip().strip(". ") == "21", "incorrect answer in the second thread"
    assert j.last_thread != trip_thread and not j.last_wire["history"], "trip history crossed threads"
    j.reopen(trip_thread, trip_run)
    j.case("PLAN-04")
