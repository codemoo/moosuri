# Calendar 대화 범위 · 합성 구현 검증

2026-11-13 구현, 2026-11-14 독립 재검토·루트 통합 수락. 구현 기준은 공개 `f2f956f`이며,
공개 PDF/볼트 변경을 포함한 `94121ca` 위에 통합했다. 아래 초기 검사와 최종 수락을 구분한다.
실제 Google 계정/OAuth/model 호출은 하지 않았다. loopback은 전송/권한/실패 검증이며 모델 이해도·지연 성적이 아니다.

## 구현

Schema30은 새 일반 chat-only profile, 상속 철회 dependency, opaque observation, 현재 요청의
봉인한 command/UUID, action link와 삭제 뒤에도 남는 최소 origin tombstone을 추가한다.
기존29 업무 해석 한 라운드에서 domain을 선택하고, 고정 알림과 일반 비후보 대화 비용을 유지한다.
직접 양식과 대화는 기존 CalendarService/26 ledger/고정 Google origin을 공유한다.

명확한 현재 사용자 단일 요청은 추가 승인 창 없이 primary timed solo-event create/update/delete를
진행한다. 누락 종료/기간, 모호한 시각/대상, 다음 페이지나 중복 제목은 변경 없이 명확화한다.
수정은 같은 날짜의 제목/시간 변경이며 날짜 간 이동·source-derived 필드·여러 턴 pronoun·미션
Calendar·반복/종일/참석자/보조 캘린더 쓰기는 포함하지 않는다. 메일 최종 사람 승인은 유지한다.

한 writer transaction에서 원래 session/epoch·봉인한 source step·연결/app·ETag·한 UUID와
실제26 action/link를 확인한다. 성공/불명/충돌의 고정 영수증은 마지막 모델 응답 없이 보인다.
불명 결과는 GET 조회만 하며 재전송하지 않는다. DELETE404는 현재 부재만 증명한다.
연결/app/source/복원 철회는 직접·상속 출력/continuation/plan을 지운다. thread 삭제 뒤 late
settlement도 origin tombstone으로 private event/payload 복원을 막고 최소 receipt만 남긴다.
로그아웃은 완료된 저장 대화를 보존하면서 미적용 원래 세션의 재시도를 막는다.

## 수행한 검증

모든 Cargo 명령에 `--offline --locked`와 루트가 공유한 기존 toolchain/vendor/직렬 target을 사용했다.
필터의 중복 검사를 합산하여 고유 시험 수로 표시하지 않는다.

| 명령 범위 | 결과 | 실제 관측 |
| --- | --- | --- |
| `cargo test -p moosuri-store calendar` | 26 통과 | ACK+plan 및 action/link rollback, 같은 UUID/원래 source step 재시도, 종료 추정0, 부분/중복 target0, 월말/시간대/DST, pre30·미션 권한 확대0, opaque paging, 상속 철회, literal auth history, token refresh/app 교체, encrypted schema30 restore |
| `cargo test -p moosuri-agent calendar` | 15 통과 | counted typed create/update/delete IO, fresh ETag/full unchanged fields, applied/unknown/conflict, 최종 model round 없는 receipt, late disconnect/logout/thread delete 최소 settlement, 기존 직접 양식 회귀 |
| `cargo test -p moosuri-agent mission_chat` | 3 통과 | 한 combined protocol round, 업무29 동작, literal reminder 새 분류0, 일반 대화 새 호출0 |
| `cargo test -p moosuri-connectors calendar` | 6 통과 | 실제 loopback query maxResults=5, overflow를 local truncate하지 않음, 정확 If-Match/notification flags/DELETE204, 재전송0 |
| `cargo test -p moosuri-server --test calendar` | 1 통과 | 기존 별도 scope/인증 및 직접 HTTP 영수증 동작 |
| `cargo test -p moosuri-store missions::chat` | 21 통과 | 기존29 해석/제어/재시도/원래 session/시각·대상 회귀 |
| `cargo test -p moosuri-store schema` | 12 통과 |30과 이전 migration rollback/retry fixture |
| affected core/connectors/store/agent/server `cargo clippy --all-targets -- -D warnings` | 통과 | 새 의존성 없이 경고 gate |
| `npm run typecheck`; CalendarEvidence/ChatPanel/Calendar/chat-state Vitest | 25 통과 | 결과 불명/현재 부재의 진실한 상태, resend/승인 UI 없음, 초안/계정 범위 회귀, malformed receipt snapshot/SSE 거부 |
| `cargo run -p xtask -- verify-plan` | 통과 | 계획 metadata와 문서 link 검사이며 제품 품질 성적이 아님 |

초기 검증에서 시험 fixture의 private helper import, 미허용 chrono test import, JSON이 아닌
checkpoint, 계획되지 않은 artifact fixture, 공유 target downgrade의 origin-table cleanup,
fixture의 local-midnight/UTC-end 변환을 수정했다. Clippy의 needless borrow/collapsible if를 고쳤다.
자체 경계 검토에서 retry의 원래 source step, 명시 IANA zone의 ACK 시점 pin, oversized 읽기의
handle 미발급, 복원 Calendar 출력 철회, thread 삭제 late private result 보존을 발견해 수정했다.
각 실제 gate의 frozen source hash/명령/log는 루트 작업 artifact의 Calendar freeze manifest를 따른다.

## 수락 경계

독립 auth/effects/storage 검토와 수리 재검토, 루트 통합 Linux 및 실제 합성 browser acceptance를
아래와 같이 완료했다. 실제 Google 동의·계정 정책·provider/device와 넓은 한국어/영어 intent
holdout/latency는 별도다. 정상 synthetic protocol 응답은 사용자 의미 해석의 정확도를 증명하지 않는다.


## 독립 검토 CALCHAT-R1 · 취소 철회 수리

원래 frozen766 source의 독립 검토는 P2 한 건을 보고했다. 이전 실행의 transient `cancelled`
상태만으로 private settlement를 막으면 Retry/claim이 `running`으로 돌아간 뒤 늦은 provider
응답이 event/payload를 다시 저장할 수 있었다. settle 직후 chat finish 전에 취소해도 이미
저장한 private event가 남았다. 원래 source에 실제 typed IO/Store 회귀 세 개를 추가하여
cancel→retry→claim→late settle, settle→cancel→finish, pre-dispatch cancel→retry를 모두 실패로
재현했다(0/3). 원래 freeze와 red log는 보존했다.

기존 미공개 schema30 action-origin tombstone의 monotonic `withdrawn` bit를 취소 transaction에서
설정하고 private payload/event/plan/관측/checkpoint/현재 출력도 함께 지운다. trigger가 bit 되돌림을
막는다. late settlement와 직접 action projection은 run.state와 별도로 이 bit를 확인한다.
현재 요청의 해석 상태는 `passed`를 유지해 계획 삭제가 새 분류/새 UUID를 만들지 않는다.
이미 확정한 UUID/nonce/state/최소 영수증은 그대로이며 새 명시적 메시지만 새 권한이 될 수 있다.
이 변경은 로그아웃·auth epoch에서 완료된 저장 역사를 지우지 않는다.

세 실제 IO 회귀는 수리 뒤 통과했다. 추가 실제 Store fault 검사에서 취소 writer 실패의 전체
rollback, payload/event/계획/출력 제거, UUID/nonce/실제 상태/최소 영수증 보존, bit 재설정 거부,
stale private event가 남았을 때도 projection 비노출과 Retry의 새 분류0을 확인했다.
수리의 최종 hash/명령과 후속 집중 gate는 별도 `calendar-conversation-r1-*` artifact를 따른다.
독립 재검토는 동일766개 파일 hash와 실제 SQLite rollback/monotonicity 검사를 확인하여
CALCHAT-R1을 닫았으며 새 중대 결함을 보고하지 않았다.

R1 최종 집중 gate: agent Calendar18, Store Calendar27, 이전 schema12, 기존 업무29 대화21,
Calendar HTTP1 및 영향받은5crate all-target Clippy `-D warnings` 통과. 수는 중복 필터를 포함하므로
합산하지 않는다. UI/DTO는 원래 freeze와 동일하며 이번 수리는 backend/SQLite 취소 경계만 바꿨다.

## 최종 Linux와 브라우저 수락

같은 수리 소스의 Linux Store Calendar27, agent Calendar18, Calendar HTTP1, restore9,
schema12, missions chat21 및5crate Clippy가 통과했다. 루트의 PDF/Calendar/SIWC 통합 소스도
Linux connectors159, private files10, mission chat3, 실제 CLI PDF3 및6crate Clippy가 통과했다.
공유 target은 하나만 사용했고 소스를 바꿀 때 입력 mtime을 갱신했다. 서로 겹치는 시험 수는 합산하지 않는다.
루트 React35·TypeScript·production build도 통과했다.

새 Chrome profile에서 실제 Rust/SQLite와 scripted loopback model을 사용해 명확화와 미지원
요청의 영수증, 응답 중 다음 초안 유지, 새로고침 뒤 저장 영수증·재분류 없음, Calendar 설정
링크를 확인했다. Google 미연결 상태에서 실제 Calendar action은0이며 성공 영수증을 꾸미지 않았다.
쓰기 증거는 별도의 counted typed IO 및 connector HTTP 검사다. 이 브라우저 검사에서 전체
새로고침은 미전송 초안을 지우는 기존 동작도 확인했다. 이는 후속 대화 연속성 수리로 남긴다.
사용한 합성 서버와 브라우저는 종료했다. 운영 서비스·실계정·CI·배포는 변경하지 않았다.
