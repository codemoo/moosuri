# Muse 중심 개인 브라우저 구조 재검토

2026-11-13 공식 공개 자료 확인. 사용자는 Muse를 주 기준, Pickle을 보조로 지정했다.
공개되지 않은 소스나 내부 client library를 추정해 구현 사실로 적지 않는다.

| 공식 관측 | moosuri에 적용하는 동작 |
| --- | --- |
| Muse는 Chromium, 전용 Linux 환경, broker가 소유하는 CDP와 제한된 브라우저 agent 인터페이스를 설명한다 | Chromium 유지, 별도 Rust broker, 개인 workspace와 typed 모델 도구 |
| Muse는 AX 관측, 사용자 takeover와 credential 입력 중 agent 정지를 설명한다 | AX + 보이는 레이아웃 텍스트, 사람만 받는 frame/input, 명시적 완료 후 현재 페이지 관측 |
| Pickle은 Kernel이 호스팅하는 원격 브라우저를 쓰고 로그인 쿠키가 남을 수 있다고 공개한다. Kernel은 Chromium 기반이다 | 작업마다 브라우저를 버리지 않고 같은 프로필에서 로그인 유지. 서버 관리 쿠키의 신뢰 경계를 표시 |
| Pickle 도움말은 직접 조작 → 다 했어요 → 같은 화면에서 이어가기, 앱 종료 후 작업 지속을 설명한다 | 채팅 카드에서 직접 조작/완료, HTTP·앱 수명과 브라우저 수명을 분리 |
| Muse connector 문서는 API가 실제 업무 완료와 재시도/결과 확인을 개선해야 한다고 요구한다 | Gmail 같은 공식 adapter와 browser를 함께 사용, 발송 승인·결과 불명 재발송 금지 유지 |

근거:

- Muse, [How We Built Safety Into Muse](https://muse.ai/blog/security-and-safety-for-ai-agents-our-approach-with-muse), 2026-09-08.
- Muse, [How we designed Muse](https://muse.ai/blog/how-we-designed-muse), [Security and privacy](https://muse.ai/security-and-privacy), [connector guidance](https://platform.muse.ai/docs).
- Pickle, [Privacy](https://pickle.com/privacy), 표시 날짜 2026-10-01. 공식 SPA 문서는 [배포 자산](https://pickle.com/assets/privacy-article-RgtjnNCV.js)에서도 확인했다.
- Pickle, [직접 조작](https://pickle.com/help/tasks/take-over-browser), [로그인 저장](https://pickle.com/help/connect/save-login), [작업 위임](https://pickle.com/help/tasks/delegate), [진행과 취소](https://pickle.com/help/tasks/check-and-cancel).
  공식 한국어 [도움말 배포본](https://pickle.com/assets/sources-Do5w8fEm.js)의 Last-Modified는 2026-11-12 23:11:54 GMT였으며 개별 문서 작성일로 해석하지 않는다.
- Kernel, [공식 문서](https://www.kernel.sh/docs): Chromium, 격리 VM, CDP/BiDi/Playwright 지원.
- Chromium, [Accessibility protocol](https://chromedevtools.github.io/devtools-protocol/tot/Accessibility/), [DOMSnapshot](https://chromedevtools.github.io/devtools-protocol/tot/DOMSnapshot/).

이 자료는 Chromium이 모든 조건에서 가장 빠르다는 비교 실험이 아니다. Playwright는
다른 브라우저 엔진이 아니라 제어 계층이며 Pickle이 그 client를 쓴다고 단정할 근거도 없다.
Rust 자체 제어와 Linux self-hosting을 유지한다. 제품 코드·로고·이미지는 복사하지 않았다.

현재 채택 범위와 아직 없는 장기 프로필·자동 credential 주입·범용 자동 입력은
[실행 계약](../browser-workspaces.md)에 분리해 기록한다. Muse의 대규모 classifier/VM
운영 체계를 전부 구현했다고 주장하거나 내부 검사를 매 클릭 승인창으로 바꾸지 않는다.
