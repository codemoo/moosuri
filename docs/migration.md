# 단계별 코드·데이터 이전

파일별 source→target은 [source inventory](../audit/source-inventory.tsv), 기능 기준은 [features](features.md), 설계 기준은 [architecture](architecture.md)다. 아래 순서는 구현 계획이며 실제 완료 범위는 [status](status.md)를 따른다. 기존 수집기·cron·DB·서비스·사용자 미완료 변경은 보존한다.

## 단계와 종료 조건

| 단계 | 먼저 읽을 기존 책임/파일 | 새 구현과 순서 | 종료 증거 |
| --- | --- | --- | --- |
| P0 공개 기반·위험 확인 | config/서비스 경계, vault CONTRACT/crypto, web PWA/UX, provider transport | OpenSpec·기여 CI·공개 개발 repo gate·새 fixture/oracle 및 pinned baseline adapter 선정→workspace·Rust MSRV/lock·React lock/DTO·dependency/SBOM; SQLite WAL/lock/FTS5, provider capability, Rig step resume, React mobile/crypto Worker spike; 최종 의존성 결정 | clean Linux build, 최소 Rust→React API, tenant proof, 기존 합성 crypto 양방향, PWA 초기 이벤트; 실패 spike를 채택으로 표시하지 않음 |
| P1 저장·격리·복구 | store/models/migrations/journal/services/run_actions/run_recovery/effects | SQLite 새 baseline·immutable blob/backup pin·offline recovery key 계약→account/tenant/context→writer/CAS/claim→request/run/checkpoint/local receipt/effect→per-tenant gate→독립 journal→usage reservation | 실제 SQLite 두 사용자/FK/FTS·concurrent request·kill/fsync/lock·삭제·finalize gap·outcome_unknown·복원 gate; 메모리 DB만으로 통과 금지 |
| P2 설치·계정·대화 | api/cli/model_settings/runtime/context, session/model/retry/thread UI tests | init/admin 하나→temp-login/force-change→user/admin React→API/OpenRouter/local/SIWC provider→model/effort→text run/SSE→cancel/retry/delete/reopen→usage dashboard | 새 설치에서 두 계정 서로 다른 대화, 기본 모델 변경/즉시 revoke, 임시 session 우회0, stream reconnect, 실제 CLI Linux 실행; Google 없이 일회 초대 전달 가능 |
| P3 자료·기억·문서 | source_protocol/bridge/adapters/retrieval/source_units/memory/extraction/workspace/obligations | Rust ingest protocol+private legacy read adapter→tenant projection/FTS→evidence/history→memory/revision/forget→bounded extraction→document/goal→obligation | cursor commit/lost ACK/full snapshot, 한국어·시간·화자·100턴/다른대화·정정·철회·재추출 방지·같은 문서 ID, 사용자별 isolation |
| P4 도구·학습·작업 | capabilities/discovery/tools/skills/learned_* /tasks/task_contract/background/runtime_effects | typed registry→필요 schema discovery→공개 절차 재작성→Git package pin/install/update/rollback·기존 제안 카드→proven route lifecycle→accepted task/plan/budget→verify/repair→effect adoption | actual linked receipts,2 distinct inputs/finalization, 귀속 실패3회, schema drift,8step/work bound, 최대2repair/동일 budget, 재시작 중복외부효과0 |
| P5 예약·소식·푸시 | schedules/scheduled_external/schedule_results/maintenance/proactive/push | 5초 due claim→occurrence/시간대/misfire→fair queue→always/changes→inbox/quiet hours→Push→고정본문 예약 effect | idle model0, active1+latest queued1, pause/wait/resume, DST·서버 중단·연속회차·이전 비교 fence, architecture의 DST/misfire/pause/edit 정책 UTC oracle, Push 계정 전환·초안 보존 |
| P6 선택 연결·메일 | collector service/state/native/calls/work_sessions, google/provider_* /parcel/public_web/assistant_connectors, deploy bridges | Google web OAuth·개인 binding·시스템 sender→gws fixture/native Gmail→초대 delivery→Gmail read/draft/send/reply→Calendar→공개검색/배송→공식 Gmail·Slack source sync→custom HTTP ingress·추적→call/session feed→선택 CLI/vehicle/maps | fresh OAuth/scopes/refresh, gws home/cache/env 분리, marker/revision/receipt/unknown, response loss 재발송0, connection/run/batch/source identity·provider checkpoint/local sequence 분리, local-only cloud fallback0; 개인 카톡 collector는 제외·custom protocol만; 미구성 adapter는 미지원 |
| P7 첨부·모바일 | media/attachment_text/mail_text, React composer/recorder/library/PWA tests | bounded upload/processor→thumbnail/object URL→image compression→PDF/media provider→녹음/편집→share/notification deep link→accessibility/preview | 업로드 중 다른 전송, failed retry 초안 보존,32MiB/48MP/2400px/2MiB×4 기준, pending cancel·late result·real iOS/Android/desktop·IME/keyboard/scroll |
| P8 독립 볼트 | vault API/store/journal/backup, vault-web crypto/import/TOTP/useVault | Rust ciphertext server·tenant store→React vault/Worker→v1 crypto→item/search/copy→CSV/TOTP→recovery/rotation/archive→independent backup→account lifecycle outbox→trusted-device credential binding/typed broker | 강한 합성 key DB/log/network canary, 기존format 양방향,nonce/envelope reservation·lost ACK/kill·partial import·회수/복원, 계정reset decrypt0, JS 위협 문구, CRED 자동 재사용/잠금 대기/수신인·범위/회수/중복0 |
| P9 private 데이터 이전·제한 전환 | 모든23 migration 계약, backup/recovery/offhost/source bridge, active deployment manifests | 아래 offline import와 drain 수행→parallel read-only 비교→명시한 범위 전환→암호화 백업·실제 restore drill | 필수 파일 manifest/pin·원래 host-local secrets 없는 빈 data_dir 복원·outer key 교체 전후/누락·파일 누락/변조/백업 중 삭제, source/target mapping·hash/count/FK·receipt/tombstone·session/credential/role/provider quarantine·accounting_unknown·latest journal·중복효과0·rollback 복구; 원본 collector/cron 유지 |
| P10 안정판 출시 | audit/기능표/테스트·운영 문서 | P0 공개 개발 repo의 허용목록/license 재확인→signed releases/SBOM→installer/container→docs/CI→실기기·성능·품질 gate | [public-release](public-release.md) 전 항목. 미검증·미지원 범위를 명시하며 실제 공개는 별도 작업 |

P2는 두 사용자 text alpha까지가 첫 수직 흐름이다. P6 Google sender가 준비되기 전에도 관리자가 임시 비밀번호를 일회 전달해 가족 계정을 만들 수 있다. P0의 crypto/mobile spike는 P8/P7 전체 구현보다 먼저 하되 본 기능의 완료와 구별한다. P9는 P1~P8의 해당 기능 gate를 통과한 범위에서만 진행한다. 실제 배포/전환을 이번 계획 작업으로 실행하지 않는다.

## private importer

1. 기존 dirty source·운영 version·schema·수집 경로를 private manifest로 고정한다. 공개 repository에는 상대 경로/해시/비민감 계약만 둔다. 원본 DB를 이번 audit에서 읽거나 덤프하지 않았다.
2. 첫 관리자 계정과 새 tenant를 만들고 이전 단일 owner의 자료를 그 tenant에 매핑한다. 새로 초대한 계정에는 아무 자료도 자동 복사하지 않는다. 기본 model/effort 등 개인 설정은 private mapping으로만 옮긴다.
3. Rust offline migration CLI가 명시된 read-only legacy PostgreSQL 또는 export를 읽어 별도 SQLite staging에 쓴다. PostgreSQL driver/tool은 optional migration build feature이며 새 설치 runtime 의존성이 아니다. 원본23 SQL을 SQLite에서 직접 실행하지 않는다.
4. source identity/version/scope·provenance·speaker/time·cursor·memory correction/forget·문서ID/revision·task acceptance·schedule occurrence·effect/receipt·학습 proof·journal watermark를 명시적으로 변환한다. 불명/불일치 행은 quarantine하며 조용히 버리지 않는다. retired 자료를 되살리지 않는다.
5. LangGraph blob·process lease·옛 login/Push/OAuth session·임시 파일/remote session을 실행 가능한 상태로 옮기지 않는다. 완료 결과와 업무 조건은 보존하고 미완료 run은 읽기 상태/재개 검토 목록으로 옮긴다. 외부 결과 불명은 같은 영수증 ID로 조회만 한다. 학습 절차 proof를 검증할 수 없으면 candidate로 둔다.
6. private media는 hash/size/MIME·소유권·참조를 확인하고 restricted data root로 이전한다. 실제 blob/CSV/토큰은 source repo로 통과시키지 않는다. 수집 cursor를 원본에 되써서 진행시키지 않는다.
7. vault는 별도 importer와 사용자 client rewrap 경로다. 기존 ciphertext와 nonce/envelope journal을 이관하고 AAD/tenant 변경 시 v1 format을 위조하지 않는다. root/recovery secret은 agent에 전달하지 않는다. 옛 journal과 새 DB의 연결을 검증한다.
8. row/count/hash·FK·고아 참조·최신 journal·검색/기억 계약을 비교한 뒤 encrypted backup+실제 isolated restore로 복구를 증명한다. hash/count만 같다고 의미 parity를 선언하지 않는다.

## 전환과 rollback

같은 외부 작업·수집 binding에 active writer가 두 개 생기면 안 된다. shadow는 read-only/synthetic이며 실제 effects는0이다. 대상 lane admission 중지→기존 task/provider process drain→DB/lock 해제 확인→manifest/최근 receipt/journal 확인→새 writer 활성화 순서다. 기존 수집기 cron은 이 계획만으로 변경하지 않는다.

code rollback과 DB rollback을 분리한다. 새로운 irreversible schema/effects가 생긴 뒤 옛 DB snapshot으로 되돌아가 재발송하지 않는다. backward-compatible binary rollback이 불가능하면 새 binary forward fix 또는 격리 restore를 사용한다. 모든 restore는 최신 독립 삭제 journal 적용·옛 password hash/session/role/provider 비활성화·미완료 실행 격리가 선행한다. 로컬 operator recovery로 현재 admin/역할과 credential을 다시 정하고 provider를 재등록한다. snapshot 이후 사용량은 independent evidence가 없으면 accounting_unknown으로 보수 유지하며 새 지출은 reconcile/명시적 새 예산 전까지 막는다. 비밀번호 변경·admin 하향·provider revoke·charge·pending reservation 뒤 snapshot 복원을 시험한다. cutover 완료 뒤 원본 정리는 참조·복구 수단·운영 무사용 증거를 확인한 대상만 단계적으로 한다.

## 검증과 작업 추적

각 구현 PR은 요구 R ID/기능 F ID, source inventory 대상, 동작 변경, 실패 시험, 실제 실행 명령과 결과를 기록한다. 파일 이동/compile 성공은 기능 완료가 아니다. Rust unit/integration + SQLite process fault + React component/browser + 실제 model 합성 평가 + 실제 기기/연결 검증을 구분한다. 계획 verifier는 표/링크/coverage만 확인하며 제품 보안·암호·품질을 증명하지 않는다.

모델 비교는 고정 업무집·동일 model/effort/provider/budget·실패 포함 분모로 실시한다. 기존40종×3회 결과는 역사적 근거이며 새 Rust 검증 수가 아니다. 품질 non-inferiority3%p 목표는 충분한 독립 표본과 구간으로 판단하고 inconclusive도 기록한다. 모델 latency와 하네스/렌더 지연을 분리한다. 체감 속도 개선을 모델/effort를 몰래 낮춰 달성하지 않는다.

신규 검사는 [testing](testing.md)과 [시나리오 표](../audit/evaluation-scenarios.tsv)의 현재 사용자 계약에서 만든다. `test-surfaces.tsv`의 옛 filename/함수 개수는 backlog가 아니며 쓸모없는 검사 재작성도 하지 않는다. 모든 F ID 연결은 누락 검사만 한다. P0의 broker spike는 P8 완료가 아니고 P6의 server-managed Google credential은 강한 vault mode와 구별한다. 주요 동작 PR은 OpenSpec delta와 관련 시나리오의 실제 증거를 함께 갱신한다.

P8의 볼트/지원 broker는 v1 제공 대상으로 남으며 선택은 사용자의 설치·연결 선택이다. P2 alpha와 P10 안정판을 구별하고, 공개 개발 저장소/기여 기반은 P0부터 준비한다. P0에서 baseline 실행/공통 fixture를 먼저 확인하므로 P10에서 비교 불가를 처음 발견하지 않게 한다. 새 복원·예약 계약은 [runtime recovery change](../openspec/changes/complete-runtime-recovery-contracts/proposal.md)에 추적한다.

## 공식·커스텀 수집 변경의 이전 순서

[build-source-connections](../openspec/changes/build-source-connections/tasks.md)의 공통 저장/인증
→custom 수신과 합성 sender→공식 Gmail/Slack→React 추적 순으로 검증한다. 기존 수동 모음은
이름만으로 외부 연결에 붙이지 않는다. private 카톡 producer의 실제 설정·데이터 전환은 별도
비공개 작업이며 공개 배포·installer가 기존 collector/cron/DB를 교체하지 않는다.
