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

## 2026-11-19 · 뮤즈 한국어 웹에서 직접 확인한 브라우저

로그인된 뮤즈 채팅에 공개 예시 사이트 `example.org`를 실제 브라우저에서 열고 조작권을 넘겨 달라고 요청했다. 채팅 메시지, 세션 주소, 스크린샷 등 개인 자료는 이 기록에 포함하지 않는다.

| 사용자 행동 | 관찰한 화면 |
| --- | --- |
| 채팅에서 사이트 열기를 요청한다 | 채팅 안에 **브라우저** 카드가 생겼다. 카드에 사이트 스냅샷, **대기 중 · 조치가 필요함**, **미리 보기 열기**가 표시됐다. 활동 기록에도 시간과 작업 요약이 남았다. |
| 카드의 스냅샷을 누른다 | 채팅 주소를 유지한 채 **브라우저 세션 뷰어**가 열렸다. 1200×1189에서는 오른쪽 768px를 사용했고 경계에 패널 크기 조절 손잡이가 있었다. 열린 상태에서 뷰포트를 390×844로 바꾸면 뷰어가 화면 전체를 차지했다. |
| 뷰어를 살펴본다 | 상단에 현재 상태, 도메인 링크, 링크 복사, 중지, 브라우저 제어, 패널 닫기가 있었다. 아래에는 열린 브라우저 창을 나타내는 탭, 창 닫기, 선택한 창의 화면이 있었다. 세션 중지와 패널 닫기는 별도 버튼이었다. 여러 창을 실제로 동시에 열어 전환하는 시험은 하지 않았다. |
| **브라우저 제어**를 누른다 | 상태가 **내가 직접 관리**로 바뀌고 세션 캔버스와 키보드 입력이 나타났다. **완료**의 접근성 이름은 ‘어시스턴트에게 브라우저 제어 권한 다시 전달’이었다. 누르자 같은 뷰어에서 **브라우저 제어** 버튼이 돌아왔다. |

뮤즈 공식 설명에 따르면 브라우저는 회원 전용 클라우드 Linux 환경에서 실행한다. 웹 작업을 맡은 하위 에이전트가 브로커를 통해 접근성 트리를 읽고 탐색과 폼 입력을 수행한다. 사용자가 조작하거나 보안 저장소가 로그인 정보를 입력하는 동안 에이전트 조작은 멈춘다. 결제는 금액과 거래 세부정보를 보여주고 승인받는다. 이번 화면 시험은 공개 페이지 열기와 제어권 왕복만 대상으로 했다. [Muse 설계](https://muse.ai/blog/how-we-designed-muse) · [Muse 브라우저 구조](https://muse.ai/blog/security-and-safety-for-ai-agents-our-approach-with-muse)

### 무수리 회원 채팅에 적용할 순서

1. 기존 Chromium 작업 공간과 직접 조작 UI를 바탕으로, Kiri 회원의 채팅에서 웹 업무를 맡기면 대화 안에 현재 상태와 작은 미리보기가 뜨게 한다. 브라우저를 다시 열 때는 그 회원과 작업에 연결된 기존 세션을 찾는다.
2. 미리보기에서 데스크톱 분할 패널 또는 모바일 전체 화면을 연다. 패널 닫기는 보기만 닫고, 작업 중지와 창 닫기는 각자의 대상에만 적용한다. 현재 도메인과 누가 조작 중인지 항상 보인다.
3. 사람이 조작하는 동안 에이전트 입력을 멈추고, 사용자가 **완료**를 누르면 원래 업무에서 같은 탭의 최신 상태를 읽고 이어간다. 앱 재진입·접속 끊김·두 기기의 동시 인계에서도 조작권은 한쪽에만 있어야 한다.
4. 먼저 공개 사이트에서 조건을 바꿔 조회하는 업무로 채팅 → 미리보기 → 직접 조작 → 에이전트 재개를 검증한다. 후속 로그인·메일 발송·결제에는 각각의 자격증명 및 최종 승인 규칙을 적용한다. [기존 구현 범위](../browser-workspaces.md)

기술 선택, Kiri 코드 진입점, 단계별 인수 조건과 시범 출시는 [회원 채팅 작업 브라우저 구현 계획](../../openspec/changes/isolate-account-runtime/browser.md)에서 추적한다.
