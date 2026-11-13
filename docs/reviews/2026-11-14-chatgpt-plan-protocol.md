# ChatGPT plan protocol · 검증 범위

2026-11-14. 공개 `752ba52` 위에 통합한 connector 전용 증분이다. 공식 SIWC의
API 요청/stream/tool 형식을 구현했으며 OAuth·개인 연결·웹 로그인·VM 전달은 별도다.
개인 계정·구독 credential·실모델 호출을 사용하지 않았다.

## 동작

기존 API client는 그대로 기본값이다. 명시적인 `ModelClient::chatgpt_plan`만 고정 공식
API origin과 별도 catalog를 사용한다. system 입력을 developer로 옮기고 미지원
`max_output_tokens`를 생략한다. 이 경로에는 원격 생성량의 hard token cap 보장이 없다.
지원 여부를 검증한 도구만 고정 `moosuri` namespace로 보내며 정확한 model·dialect·namespace·
name·call ID를 private continuation에도 고정한다. 잘못된 namespace나 다른 dialect를 거부한다.
실패·불완전·중간 text·usage 관측은 성공과 구분하고 clean HTTP EOF 전에는 도구를 실행 가능하게 돌려주지 않는다.

## 독립 검토와 수정

P2-1은 정확한 응답 identity를 확인하기 전에 failed frame 사용량을 보존할 수 있고,
foreign progress identity를 이후 정상 terminal이 덮을 수 있다는 결함이었다. plan dialect에서
실패 사용량은 검증한 created 이후 같은 response ID/model일 때만 보존하도록 수정했다.
progress/queued/full terminal도 그 identity를 검사한다. Ordinary API 동작은 바꾸지 않았다.
수리 후 독립 재검토는 원래13개 파일 범위·patch/hash·실제 시험 로그를 확인하고 해당 결함을 닫았다.

## 확인한 결과

- Native connector158개와 all-target Clippy, fmt, 고정 OpenSpec 검증 통과.
- 실제 loopback HTTP: fragment, 정확 request/catalog, 두 라운드 도구/continuation,
  실패 전 text 유지, 외부 response 사용량 비보존, held EOF·잘못된 terminal 거부.
- PDF/Calendar를 합친 root Linux source: connector159개, private files10개,
  mission chat3개, CLI PDF3개, 영향받은6crate all-target Clippy 통과.
- 공개 전 source/commit secret scan, plan metadata, OpenSpec 및 fmt 확인.

수는 겹치는 필터를 포함하므로 합산하지 않는다. 별도 의존성·schema·공유 provider 권한은
추가하지 않았다. 원래 freeze와 실패/수리 로그는 private 작업 증거로 보존했다.
실제 모델 목록·구독 eligibility·text/tools capability, OAuth/JWKS·원자적 refresh와 개인
소유 binding·Claude runtime은 이 결과로 완료 처리하지 않는다. CI와 운영 배포는 변경하지 않았다.
