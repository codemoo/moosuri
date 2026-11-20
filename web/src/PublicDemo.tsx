import { useState } from "react";

const examples = [
  { name: "시간표 확인", question: "오후 3시 12분 이후 첫 노을 열차는 몇 시야?",
    answer: "오후 3시 40분, 7번 승강장에서 출발해요.", evidence: "예시 시간표 · 15:10 / 3번, 15:40 / 7번" },
  { name: "기억 정정", question: "내가 즐기는 차를 보리차에서 현미차로 바꿔 기억해 줘.",
    answer: "즐기는 차를 현미차로 바꿨어요.", evidence: "예시 기억 · 보리차 → 현미차" },
  { name: "맡긴 일", question: "아까 맡긴 시간표 확인은 어떻게 됐어?",
    answer: "조회가 끝났어요. 노을 열차의 출발 시각과 승강장을 원래 대화에 남겼어요.", evidence: "예시 작업 기록 · 요청 → 시간표 확인 → 완료" },
];

export function PublicDemo() {
  const [selected, setSelected] = useState(0);
  const [draft, setDraft] = useState("");
  const example = examples[selected];
  return <main className="public-demo">
    <header><a className="demo-brand" href="/demo"><img alt="" src="/moosuri-icon-192.png" width="32" height="32" />Moosuri</a>
      <a href="/app">회원 공간 열기</a></header>
    <section aria-labelledby="demo-title">
      <h1 id="demo-title">예시 대화</h1>
      <p>합성 자료로 무수리의 답변과 작업 결과를 살펴보세요.</p>
      <nav aria-label="예시 선택">{examples.map((item, index) => <button type="button" key={item.name}
        aria-pressed={index === selected} onClick={() => setSelected(index)}>{item.name}</button>)}</nav>
      <div aria-live="polite" className="demo-exchange">
        <p className="demo-question">{example.question}</p>
        <div className="demo-response"><p>{example.answer}</p><small>{example.evidence}</small></div>
      </div>
      <label htmlFor="demo-draft">입력 연습</label>
      <textarea id="demo-draft" rows={2} placeholder="여기에 예시 질문을 써 보세요." maxLength={2000}
        value={draft} onChange={event => setDraft(event.target.value)} />
      <small>작성한 내용은 이 화면을 떠나면 지워집니다.</small>
    </section>
  </main>;
}
