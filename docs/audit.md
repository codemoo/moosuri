# 전수 조사·정리 결과

2026-11-12 기존 checkout의 **비생성 파일523개**를 고정해 경로·크기·SHA256·처분·새 target·단계·기능·근거·시험 계약을 기록했다. AST/import/CLI/배포 참조·테스트·문서·UI/볼트·수집 연결 경로를 함께 확인했다. 모든 줄의 독립 보안 검토나 새 제품 실행 시험을 마쳤다는 뜻은 아니다. DB·credential·통신 원문을 audit 입력으로 읽지 않았다.

## 범위와 산출물

| 범위 | 비생성 파일 수 |
| --- | ---: |
| agent | 286 |
| web | 62 |
| vault-web | 39 |
| vault | 21 |
| 수집 src | 19 |
| 수집 tests | 6 |
| deploy | 33 |
| docs | 50 |
| requirements | 2 |
| root 진입·패키징 | 5 |
| 합계 | 523 |

- [source inventory](../audit/source-inventory.tsv): 파일마다 하나의 판정. 새 target은 예정 경로이며 존재/구현 완료를 뜻하지 않는다.
- [source manifest](../audit/source-manifest.tsv): 고정 경로·hash·bytes. inventory와 전수 대조한다.
- [entrypoints](../audit/entrypoints.tsv): HTTP route 정의59개, model tool 정의49개. 설치별 활성 endpoint 수와 다르다.
- [test surfaces](../audit/test-surfaces.tsv):99개 test source 파일, Python named test 함수772개·TypeScript 직접 test 선언215개.

TS 집계는 `test(...)`/`it(...)`의 literal title 선언을 대상으로 한다. 단순 `test(` 검색221개에서 정규식 객체의 `.test(...)` 호출6개를 제외해215개로 정정했다. loop/parametrize/project 조합으로 실행 수가 바뀌므로 기존 backend1,121/UI150 결과와 같지 않다. Python은 전체 AST의 test_ 함수 정의를 센다. 기존 결과는 역사적 증거이며 Rust 검증 수가 아니다.

원본 경로 중 개인 domain을 포함한 배포 template 파일명1개는 `web/deploy/PRIVATE_ORIGIN.conf.in`으로 가렸다. hash/bytes와 private 원본 mapping으로 같은 파일임을 추적한다. 공개 inventory에 개인 운영 주소나 원문을 넣지 않는다. 비공개 상세 조사/dirty status/원래 mapping은 작업용 private audit에만 보존하며 공개 패키지 의존성이 아니다.

## 처분 의미

| 판정 | 파일 수 | 뜻 |
| --- | ---: | --- |
| rewrite | 148 | 동작을 새 Rust 또는 React 구조에 재작성 |
| contract | 40 | 스키마·프로토콜·평가 기준의 동작 근거. 테스트 파일 자동 이식은 제외 |
| reference | 140 | 설계/화면 등 근거만 참조, 원문 그대로 반입하지 않음 |
| exclude | 127 | 이전 실험·private 지침·운영 증적·hosting 통합을 새 source에서 제외 |
| optional | 2 | 선택 provider/inference 책임. 미구성/중지 상태를 dead라고 판정하지 않음 |
| replace | 66 | 패키징·설치·자산·진입점 등을 새 일반 구성으로 대체 |

이 판정은 **새 프로젝트 반입 결정**이다. 원본523개 파일을 삭제했다는 의미가 아니며 운영 파일 물리 삭제 허가도 아니다. 기존 dirty changes·수집기·cron·source DB·vault·production은 그대로 둔다. 새 계획 폴더에는 원본 runtime/source/history를 복사하지 않는다.

## 확인한 불필요·중복·dead 후보

| 대상·근거 | 처리 |
| --- | --- |
| `agent/src/musuri_agent/store.py:342`의 memories 분기. 같은 함수312~315에서 MemoryService 위임 후 return | 증명된 unreachable block. 새 구현에 옮기지 않음; 원본 전체 store 삭제 근거 아님 |
| `skills.skill_context`, `LearnedSkillService.context`는 test call만, production은 combined selector | 통합 후보. 외부 API compatibility 확인 전 dead 단정/원본 제거 금지 |
| CLI/services/backup_export/offhost/provider/source bridge는 일반 import만 보면 미사용 | pyproject entry·동적 import·`-m`·installer 참조 확인. 활성 책임을 유지 |
| `web/scripts/vite/sites-vite-plugin.ts`, `.openai/hosting.json` | Vite plugin/closeBundle에서 실제 사용. 새 일반 self-host에는 불필요하여 제외; 옛 build에서 미사용인 것은 아님 |
| 옛 Next 설정·`web/.next/types`, `web/next-env.d.ts` | 새 React/Vite 설정으로 대체. 생성 산출물은 source523 집계 밖이며 새 tree에 반입0 |
| preview/model/샘플·screenshots | 현재 참조·시험이 있음. demo 기능은 유지하고 합성 자료/자체 자산으로 재작성 |
| Hermes/OpenClaw 후보 runtime·probe·freeze·과거 출력 | 과거 시험 증거만 보존. 새 runtime 의존성/제품 source에 넣지 않음 |
| 비공개 뮤즈 메일/PDF 유래50개 절차·개인 관례 | catalog 원문 반입 제외, 공개 요구/자체 동작으로 재작성 |
| Todoist paused·지도 unconfigured·Mac provider 중지·local STT | optional/unavailable과 dead 구별. Linux provider가 재사용하는 코드도 있어 통째 삭제하지 않음 |
| 개인 도메인/host/UID/path·수집 설치 Python | 공개 기본값 제외, Rust CLI/일반 config와 private migration mapping으로 대체 |

71개 agent Python 모듈(13,797줄) 중 통째로 dead라고 입증한 모듈은 없었다. source별 target은 소스 모듈 수를 새 crate 수로 옮기라는 뜻이 아니다. 공통 ID/revision/error/DTO·worker lifetime·도구 wrapper·provenance 검사를 통합하되 hash/canonicalization/timezone/직접·전이 참조 의미는 회귀로 비교한다.

## 수집 경로의 구별

기존 원본 운영 수집기, checkout의 schema2 보존 수집 코드, 현재 private read-only bridge→Linux projection은 서로 다른 경로다. 계획은 실제 active collector를 임의 교체하지 않는다. source identity·scope·cursor-after-commit·lost ACK 재전송·complete snapshot retire·독립 journal을 먼저 Rust 연결 계약으로 옮긴다. Google/차량 effects가 Linux에서 실행된다는 사실과 Mac 읽기 bridge를 구별한다.

## 문서 정본과 검증 한계

진입은 AGENTS→README→필요한 상세 문서, 변경 가능한 상태는 [status](status.md) 한 곳이다. CLAUDE는 같은 정본으로 연결한다. 옛 운영 문서를 새 지시로 복사하지 않고 역사적 증거로 남긴다. file inventory·manifest는 이번 source snapshot 기준이며 새 버전 조사 시 기존 evidence를 덮어쓰기보다 새 baseline을 명시한다.

`cargo run --locked -p xtask -- verify-plan`은523 경로/해시/크기 대조·필수 필드·단계/F ID·R01~R29/F01~F70 정의·새 평가 시나리오/feature 연결·108진입점·정적 시험 수·로컬 문서 링크를 검사한다. inventory 의미의 정확성·전체 보안·암호·제품 동작·최신 외부 문서의 진실성을 증명하지 않는다. 이 부분은 독립 검토와 향후 구현별 시험으로 확인한다.

## 기존 테스트 재선별

사용자 지시에 따라112개 test/support는108개 reference·4개 exclude로 변경했다. 제외한 네 파일은 `vault-web/tests/integration/{api_server.py,run.py,static_proxy.mjs}`와 `vault-web/tests/static-server.mjs`의 옛 실행 wrapper다.99개 test source의 선언/line 좌표는 조사 증거로 보존하고 target은 새 Rust 파일명이 아닌 참고 정책으로 바꿨다. `source-manifest.tsv`의 원본 hash/bytes는 바꾸지 않았다.

새 테스트는 [testing](testing.md)의 실제 결함/사용자 결과로 선정한다. 기존 helper·mock 호출 순서·prompt wording·중복 CRUD는 새 구현의 의무가 아니다. 과거 사고의 유효한 실패 조건만 새 경로로 재현하고 기능별 누락은 [새 시나리오](../audit/evaluation-scenarios.tsv)로 확인한다.

## R29에 따른 수집 target 변경

2026-11-13부터 카톡 collector는 exclude, 카톡 전용 retrieval은 공통 thread/provenance의
contract만 남긴다. shared dispatch/config/bridge의 Kakao-specific 분기도 공개 target에서
제외한다. source hash/bytes/원래 evidence와 manifest는 그대로 보존한다. 기존 운영 파일을
삭제하지 않는다. [source-connectors](../audit/source-connectors.tsv)와 [수집 계약](data-connections.md)이
공식/custom 계획과 개인 전용 제외의 정본이다. 추적표는 planned/implemented/excluded를 구별한다. implemented는 실제 code target과
별도 검증 기록을 요구하며, 그 기록의 합성/실계정/플랫폼 검증 범위를 구별한다. Rust 계획 검증은 분류/배포/상태의
일치, feature/scenario 참조, 안전한 상대 target/spec 경로와 카톡 제외를 검사한다. metadata 연결을 검사할 뿐
수집 runtime/API가 구현됐다고 증명하지 않는다.
