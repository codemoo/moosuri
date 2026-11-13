# 기능별 이전·검증표

이 표는 전체 기능의 요구 계약이다. 실제 구현·검증 완료 범위는 [현재 상태](status.md)를 따른다. `기본`은 v1 필수, `선택`은 사용자가 해당 연결/부가 패키지의 설치·사용을 고르는 지원 계약이며 자동으로 출시 연기를 뜻하지 않는다. 볼트 F44~F51/F63와 credential F69는 v1에서 제공해야 하는 선택 설치 패키지다. 외부 연결의 실제 지원 범위는 provider별 gate로 표시한다. `후속`만 v1 제공 약속 밖의 별도 가능성 검증 범위다. 소스 좌표는 이전 저장소 기준이며 원문을 공개 코드로 복사하라는 뜻이 아니다. 실제 파일별 판정은 `audit/source-inventory.tsv`와 연결한다.

## 설치·대화·웹

| ID | 기능·구분 | 원본 근거 | 새 목표·단계 | 통과해야 할 동작 |
| --- | --- | --- | --- | --- |
| F01 | 새 설치·진단·설정 · 기본 | `config.py`, `cli.py`, `deploy/linux/` | CLI/server · P0/P2/P10 | 개발자 계정 없이 init, 로그/일반 출력에 secret0·setup UI/TTY 일회 전달, origin/data-dir 설정, 연결 없는 시작, doctor 고정 오류 |
| F02 | 로그인 유지·기기 세션 · 기본 | `api.py`, `models.py`, `020_session_continuity.sql`, web auth/push tests | server/web · P2 | HttpOnly/SameSite/Origin, 기본30일·활동 갱신, 실제401만 잠금, logout/relogin 응답 순서, 모든 기기 폐기 |
| F03 | 채팅 생성·목록·삭제 · 기본 | `thread_lifecycle.py`, `store.py`, `thread-delete.spec.ts` | store/web · P2 | 현재/이전/마지막 대화 삭제, pending upload·실행 fence, 오래된 페이지/SSE로 재등장하지 않음; 저장 기억/문서/예약은 별도 |
| F04 | 대화 연속성·오래된 문맥 · 기본 | `context.py`, `workspace_tools.py`, `test_memory_context_adoption.py` | agent/web · P2/P3 | 마지막 대화 복귀, 이전 페이지 scroll anchor, 같은 대화 지시어·100턴 전/다른 대화 검색, 새 질문과 구분 |
| F05 | 모델·effort 선택 · 기본 | `model_settings.py`, `012_model_settings.sql` | ModelPort/server/web · P2 | 새 실행부터 변경, 재개는 기존 설정, model capability 확인, 설정 충돌 보존, 키는 서버만 |
| F06 | 자연스러운 스트림·중단 · 기본 | `runtime.py`, `runtime_errors.py`, `model-streaming.spec.ts` | agent/server/web · P2 | chunk/event 순서, first-final-body, 중간 말과 최종 답 구분, reduced motion, 취소 후 늦은 본문 차단 |
| F07 | 실패·재시도·중복 방지 · 기본 | `run_recovery.py`, `run_actions.py`, `019_run_recovery.sql`, `retry.spec.ts` | core/store/agent/web · P1/P2 | 같은 message/run/attachment로 재개, composer 초안 보존, attempt 확인, lost ACK·연속 클릭·완료 checkpoint gap |
| F08 | 네트워크·탭·철회 연속성 · 기본 | `api.py`, `workspace_invalidation.py`, `continuity.spec.ts` | server/web · P1/P2 | focus/online/bfcache에 최신 상태/cursor 읽기, 늦은 취소·logout·삭제·epoch 응답 차단 |
| F09 | 채팅 중심 UI·모바일·접근성 · 기본 | `web/app/`, `mobile-input.spec.ts`, `agent.spec.ts` | web · P2/P7 | 넓은 작업 영역, bottom safe area, 독립 scroll, IME Enter 중복 전송0, visual viewport keyboard, focus/aria/reduced motion |
| F10 | 사진/파일 첨부·전송 독립 · 기본 | `media.py`, `web/app/`, `test_media.py` | connectors/web · P7 | 업로드 중 다른 글 전송, pending 선택 유지, 취소 UUID tombstone, processing gate, 같은 bytes 재시도, per-file 제한 |
| F11 | 사진 미리보기·보관함 · 기본 | `media.py`, `workspace.py`, web UX tests | server/web · P3/P7 | composer/말풍선/library thumbnail·확대, 인증 no-store object URL, 삭제/화면전환/세션 폐기 시 취소·해제 |
| F12 | 큰 이미지 최적화 · 기본 | web image preparation·UX tests | web/connector media · P7 | 기존32MiB/48MP 입력 상한·2400px 압축·2MiB 업로드 계약을 먼저 검증; 포맷/EXIF/해상도 폭탄·실패시 이유 |
| F13 | 녹음 인터페이스 · 기본 | web recorder·UX tests | web · P7 | 권한 요청, pause/resume, 시간·입력 상태, preview/re-record/delete, 통화/화면전환 중 track 정리 |
| F14 | 음성·이미지·PDF 텍스트 처리 · 기본 | `attachment_text.py`, `media.py`, `mail_text.py` | connectors/agent · P3/P7 | 처리 완료 뒤 답변, STT는 초안 확인 후 전송, page/bytes/time 상한, lease 만료 후 늦은 처리 금지; codec 불가를 숨기지 않음 |
| F15 | 문서·목표 편집·내보내기 · 기본 | `workspace.py`, `workspace_tools.py`, web editors | agent/server/web · P3 | same ID/revision 갱신, 충돌 초안 보존, 근거 철회, Markdown·스크립트 차단 print HTML |
| F16 | PWA 설치·공유·offline shell · 기본 | `web/public/service-worker.js`, manifest, `pwa.spec.ts` | React web + TypeScript service worker · P0/P7 | public shell만 cache, private offline queue 없음, one-use 공유 초안, TTL·worker 재시작·로그인, 자동 전송0 |
| F17 | Web Push·알림 열기 · 기본 | `push.py`, `proactive.py`, web push/service-worker | effects/web · P0/P5/P7 | 최초 gesture 동의, 기존 endpoint repair, 멀티탭 세대, 일반 payload+opaque ID, 클릭 초안 보존, 만료 endpoint 제거 |
| F18 | 공개 demo/미리보기 · 기본 | `web/app/preview.tsx`, `model.ts`, preview tests | web/synthetic fixtures · P7/P10 | 개인 API와 완전 분리, 샘플이 개인 공간에 섞이지 않음, 공개 screenshot에 합성 자료만 |

## 자료·기억·도구·작업

| ID | 기능·구분 | 원본 근거 | 새 목표·단계 | 통과해야 할 동작 |
| --- | --- | --- | --- | --- |
| F19 | 직접 기억·검색·편집 · 기본 | `memory.py`, `memory_tools.py`, `memory_temporal.py` | agent/store/web · P3 | 현재 유효 사실, speaker/subject/source/time, 수정 revision, 검색/filter/editor 복귀, 미래 사실을 현재로 사용하지 않음 |
| F20 | 자동 기억 추출 · 기본 | `extraction.py`, `source_units.py`, `memory_temporal.py` | agent/data worker · P3 | original 발화/인용 단위, bounded daily call/token, changed/accessed 우선, 불명 화자 제외, 과거 전체 무단 backfill 없음 |
| F21 | 망각·정정·자료 해제 · 기본 | `journal.py`, `memory.py`, `source_bridge.py`, invalidation tests | store/agent/server · P1/P3 | source keep/delete, memory forget, thread delete 범위 구분; replay/캐시/checkpoint/SSE/산출물 tombstone·epoch |
| F22 | 원문 검색·근거·페이지 · 기본 | `retrieval.py`, `source_adapters.py`, `tools.py`; thread identity 공통 계약만 참조 | agent/connectors · P3 | 한국어 동의어·FTS5/부분문자열 비교, 방/화자/시각/파일 범위, 실제 id/version 인용, partial pagination, 동명이인/본인후속답장 |
| F23 | 공식·커스텀 수집 원본·cursor·batch · 기본 | `src/musuri/{service,sources,state}.py`, `source_protocol.py` | connectors/store · P1/P3/P6 | batch commit 후 cursor, scope identity, exact replay no-op, lost ACK same receipt, full snapshot만 범위 retire, heartbeat와 자료 변경 구분 |
| F24 | 도구 선택·capability · 기본 | `capabilities.py`, `tool_discovery.py`, tools modules | agent/connectors · P4 | 실제 연결/권한/상태 반영, metadata/schema 선택, 저장 discovery 복원, unknown 등록 도구 누락 방지, 호출 상한 |
| F25 | 일반 절차 catalog · 기본 | `skills.py`, `skill_library/` | agent/resources · P4 | 공개 요구사항으로 재작성, 필요한 본문만 읽기, 입력·한계·실제 도구 명시; private 유래50개 원문 반입0 |
| F26 | 성공 절차 학습·재사용 · 기본 | `learned_skills.py`, `learned_routes.py`, `learned_usage.py`, migration022 | agent/store · P4 | 9도구/8단계, 실제 linked receipts, 2 distinct inputs, finalization gap, schema drift, owner edit/delete, 실패3회, bounded match |
| F27 | 복합 작업·계획·검증 · 기본 | `background.py`, `tasks.py`, `task_contract.py`, `workspace.py` | agent/tasks worker · P4 | 원래 조건·acceptance version, 산출물 검증, 최대2repair/같은 예산, pause/resume/steer/extend 분리, 완료 과장0 |
| F28 | 목표·요청·약속·회신 대기 · 기본 | `obligations.py`, `obligation_context.py`, `proactive.py` | agent/web · P3/P5 | requested/committed/waiting/resolved/cancelled, 부분 완료/위임, mute/snooze, 최신 유효 추적 최대5개 briefing |
| F29 | 자연어 예약·5초 due 조회 · 기본 | `schedules.py`, `task_tools.py`, `maintenance.py` | agent/tasks worker · P5 | 일회·일/주/간격·명시 DST gap/fold·UTC anchor, default task vs alarm, idle LLM0, 최소60초, unique occurrence·latest queue coalescing |
| F30 | 예약 제어·장애 복구 · 기본 | migration020, `tasks.py`, recurring tests | agent/store/web · P5 | active1+queued1, waiting/pause 유지, resume/수정 회차 보호, 일회 종료·latest-only misfire·pause 기간 skip·edit revision fence, architecture 정책표의 UTC oracle |
| F31 | 변경 있을 때만 알림 · 기본 | `schedule_results.py`, migrations021/023, `work-cards.tsx` | agent/store/web · P5 | always/changes, 최초/변경/실패/unknown 알림, strict equal만 suppress, 이전 output 비교 fence·resume 승계, 연쇄 의존 누적0 |
| F32 | 소식·제안·quiet hours · 기본 | `proactive.py`, `push.py`, `followups_*` | agent/effects/web · P5 | 읽음/숨김, topic mute, 조용한 시간, 실제 유효 참조, inbox와 Push 중복 정책, 자동 제안 과장0 |
| F33 | 외부 작업 원장·복구 · 기본 | `effects.py`, `runtime_effects.py`, `provider_dispatch.py` | core/store/effects · P1/P4/P6 | owner authority·checkpoint adoption·target revision·same effect ID, lost ACK→조회, 취소 후 영수증 보존, 미확정 재발송0 |
| F34 | Gmail 읽기·자료 수집 · 선택 | `native.py`, `google_provider.py`, `mail_text.py` | connectors/google · P6 | 설치자 OAuth, history invalidation fallback, message/thread/PDF/attachment pagination, fixed hosts·token redirect 금지, 원문 identity |
| F35 | Gmail 초안·발송·답장 · 선택 | `google_provider.py`, `provider_service.py`, effects tests | connectors/effects · P6 | draft create/update/delete/send+readback, 명시된 단일 수신인·내용, marker/Message-ID, unknown 조회; reply-all/첨부발송은 현재 미지원 |
| F36 | 고정 내용 예약 발송 · 선택 | `scheduled_external.py`, `task_contract.py` | agent/effects · P5/P6 | owner-fixed 수신인/본문/시각·occurrence identity,5분 lateness expiry, restore 재승인 없는 재실행0; 미래 생성 내용 자동발송 제외 |
| F37 | Calendar 읽기·쓰기 · 선택 | `native.py`, `google_provider.py`, calendar tests | connectors/google · P6 | calendar/account/range scope, partial read로 삭제0, ETag/revision, 소유권 확인한 calendar만 writes, 기존 primary 계약 |
| F38 | 개인 카톡 수집기 · 공개 배포 제외 | `src/musuri/collectors/kakao.py`, `kakao_threads.py`; R29로 범위 변경 | 별도 private producer → F70 custom protocol · P6/P9 | public 코드/이미지/installer/카톡 전용 tool0; 기존 개인 collector/cron/DB 보존; generic scope·thread identity 계약만 재사용 |
| F39 | 공식 Slack 읽기·수집 · 선택 연결 | `native.py`, `source_adapters.py` | connectors/slack · P6 | 대화·thread pagination/late reply·cursor·rate limit, 허용 범위, bot token/계정 분리; Slack 쓰기 기본 없음 |
| F40 | 통화 원본·전사 feed · 선택 | `collectors/calls.py`, `native.py`, source units | connectors/calls · P6/P7 | settled file·hash dedup·changed file defer·bounded transcription, transcript receipt, 화자 불명 자동 기억 제외; 전화 발신 없음 |
| F41 | 작업 세션 수집 · 선택 | `collectors/work_sessions.py`, `source_adapters.py` | connectors/local-session · P6 | opt-in 디렉터리, inode/fingerprint·partial·finished bucket, automation/subagent 제외, 사용자 요청만 evidence; tool 실행0 |
| F42 | 공개 웹 검색·배송 · 선택 | `public_web.py`, `parcel.py` | connectors/public · P4/P6 | 실제 hosted search receipt/URL·usage, 일본우편 공식 이력/parser errors, 운송장 slot 재사용; 없는 배송사 연결 완료 주장0 |
| F43 | 차량·지리·Todoist·local STT · 후속 | `assistant_connectors.py`, `native.py`, `requirements/` | optional connectors · P6 이후 | private vehicle proxy를 기본값에서 제외, routing 미연결·Todoist paused를 dead로 분류하지 않음, crate/API/권한/품질 gate 후 제공 |

브라우저 관련 F09/F24/F27/F42/F58의 현재 구현 부분은 [개인 작업 브라우저](browser-workspaces.md)에
있다. 사용자·대화별 프로필, 화면 보기, 사람의 로그인/조작과 같은 페이지에서의 후속 대화를
연결했다. 범용 자동 폼·장기 프로필 복구·배송사 adapter나 해당 기능표 전체 완료를 뜻하지 않는다.

## 볼트·운영·공개

| ID | 기능·구분 | 원본 근거 | 새 목표·단계 | 통과해야 할 동작 |
| --- | --- | --- | --- | --- |
| F44 | 암호 볼트 독립 경계 · v1 선택 설치 | `vault/CONTRACT.md`, vault api/config, web vault link | vault binary/server/web · P0/P8 | 별도 origin/DB/cookie·로그인/전체기기폐기·lock/idle/focus, 비서 plaintext/key 접근0, vault 부재해도 기본비서 동작 |
| F45 | 암호화·worker 수명 · 선택 패키지 | `vault-web/src/crypto/`, `CRYPTO_DEPENDENCIES.md` | vault-client worker · P0/P8 | Argon2id13/AES256GCM/HKDF/AAD/nonce 기존 v1 호환, OS entropy, root는 worker만, generation/epoch fence, invalid-before-KDF |
| F46 | 항목·검색·암호 생성·복사 · 선택 패키지 | vault main/useVault/crypto | vault-client/web · P8 | login/API-key/note/favorite/expiry, 암호화 검색 summary, revision conflict, RNG rejection sampling, late copy/reveal/lock races |
| F47 | Apple CSV 가져오기 · 선택 패키지 | `vault-web/src/import/`, import tests | vault-client/import · P8 | 로컬 parse·BOM/CRLF/quote·행/byte 상한·선택/중복·cancel/partial commit, username filter 사용자 입력, 특정 개인 ID 하드코딩0, CSV 서버전송0 |
| F48 | TOTP · 선택 패키지 | crypto TOTP·`TotpCard.tsx`, `totp.test.ts` | vault-client · P8 | otpauth URI/Apple fallback·알고리즘/digits/period·worker clock, 만료/초점/항목변경·copy 재계산, seed/code 로그·summary 포함0 |
| F49 | 비밀번호·복구키·root 교체 · 선택 패키지 | vault envelopes/rotate·journal/reservations tests | vault/client/store · P8 | immutable retained envelopes, new epoch/session revocation, loss ACK/reconcile, durable candidate·nonce ledger, 이전 wrapper 부활0 |
| F50 | 독립 암호화 내보내기/가져오기 · 선택 패키지 | [archive 계약](vault-archive.md)·core/Worker/React | vault-client · P8 | manifest tamper/reorder/truncate 탐지, 원래 서버 없이 복원·새 IDs·현재 key, 과거 archive wrapper 수명 한계 명시 |
| F51 | 볼트 backup·복원·복제 · 선택 패키지 | `vault/{backup,journal,offhost}.py`, `deploy/vault/` | vault/CLI · P8/P9 | ciphertext도 별도 운영자 recipient로 outer encryption·old identity 보관·개인 master와 분리, 독립 latest journal, nonce/envelope 예약 복원, staged state 없으면 recovery_required, 실패 restore quarantine 유지 |
| F52 | 비서 journal·backup·restore · 기본 | `journal.py`, `backup.py`, `offhost_backup.py`, `recovery.py` | store/CLI · P1/P9 | fsync/hashchain/head, snapshot watermark, offline backup identity/key ID·교체 전후 복원·key 누락 오류, encrypted pipe, SQLite online backup·WAL·필수 blob manifest/pin/암호화, 빈 data_dir 복원·누락/변조 거절, fresh quarantine DB, sessions/jobs/effects 격리, 최신 삭제 우선 |
| F53 | 배포·drain·업데이트·제거 · 기본 | `deploy/linux/`, macOS installers, `services.py` | CLI/deploy · P0/P9/P10 | manifest/hash, 하나의 writer, DB 종료 확인·forced wrapper lock, forward schema, 코드 rollback/DB rollback 구분, 제거 기본은 data 보존 |
| F54 | 진단·측정·평가 · 기본 | `metrics.py`, `agent/evaluations/`, all tests | test-support/xtask · P0~P10 | 단계 지연·사용량·실패 분모, synthetic/live/device 구분, disabled tracing, 원문 없는 export diagnostics, skipped DB tests를 통과로 세지 않음 |
| F55 | 공개·기여·라이선스·재현 · 기본 | 기존 docs/locks/asset provenance | docs/xtask/deploy · P10 | 새 Git 이력·허용목록·비밀/권리 검사, signed release/checksum/SBOM, clean install/update/restore, 지원 범위·오류 보고 템플릿 |

## 추가 설치·다중 사용자·provider 계약

| ID | 기능·구분 | 근거 | 새 목표·단계 | 통과해야 할 동작 |
| --- | --- | --- | --- | --- |
| F56 | 관리자·초대·임시 비밀번호 · 기본 | R04/R06~R11, onboarding | server/React admin · P1/P2/P6 | 최초admin1, 이름/이메일, skip 임시 비번, force-change 제한 session, invite expiry/reissue/unknown send, self-signup 기본0 |
| F57 | 공용 모델·개인 사용량 · 기본 | R05/R11/R12 | provider/store/admin · P1/P2 | shared+user reservation 원자성, fair queue, 실제/추정/불명 정산, model/effort version·즉시 revoke, admin private 내용0 |
| F58 | 사용자별 데이터·환경·세션 · 기본 | R16, accounts-isolation | core/store/server/agent/web · P1~P10 | context/FTS/FK/cache/SSE/remote ID/home/credential 격리, restart/reopen, cross-user IDOR0, account switch late results0 |
| F59 | OpenAI·Anthropic API·공식 구독 경로 · 기본/선택 | R04/R05, providers | ModelPort/isolated CLI · P0/P2/P6 | API 직접 연결, SIWC 사용자별 동의/host/history 제한, Claude token 중계0·공식 CLI 별도 gate, unsupported 상태 정확 |
| F60 | OpenRouter·로컬·compatible LLM · 기본 연결 지원 | R17/R18, providers | connectors/model/admin · P0/P2 | Ollama/LM Studio/vLLM preset, 실제 tools/stream/effort/usage probe, local-only 외부 전송0, endpoint DNS/redirect 경계 |
| F61 | SQLite 저장·설치 · 기본 | R15, architecture | store/CLI · P0/P1/P9 | WAL/FULL/FK, single writer·CAS·boot lock, 실제 kill/backup/restore, 네트워크 FS 금지, 새 설치 PG필요0 |
| F62 | Tailscale·일반 HTTPS · 기본 | R13, onboarding | CLI/deploy · P0/P2/P10 | Serve private/Funnel public 구별, app auth 유지, spoof header 거절, OAuth callback·SSE·별도 vault origin·mode 전환 |
| F63 | 사용자 키 볼트·관리자 경계 · 선택 패키지 | R14, vault-security | vault/client/store · P0/P8 | DB/source admin decrypt0, no escrow·reset≠decrypt, 악성 웹 JS 한계 표시, 강화 client는 독립 검증 후 지원 |
| F64 | 다중 사용자 삭제·정지·복원 · 기본 | R16, accounts-isolation | store/CLI/vault outbox · P1/P8/P9 | per-user journal/auth epoch, session/invite/Push/jobs/effects 부활0, 두 DB 삭제 ACK/pending, 다른 사용자 영향0; restore 옛password/role/provider 재활성화0·사용량불명 새지출0 |

## 기여·공유·credential 자동 연결

| ID | 기능·구분 | 근거 | 새 목표·단계 | 통과해야 할 동작 |
| --- | --- | --- | --- | --- |
| F65 | contributor/OpenSpec 공동 개발 · 기본 | R25, CONTRIBUTING | docs/openspec/CI · P0/P10 | 고정 도구·작은 delta·독립 경계 검토·fork secrets0·계획/실제 완료 구별 |
| F66 | community 스킬 설치·버전·제거 · 기본 | R26, Agent Skills/Vercel/ClawHub | Rust skill manager/store · P4 | source+commit/hash pin·per-user enabled state·atomic update/rollback·수정 보존·disable/remove schedule 처리 |
| F67 | 스킬 작성·학습 절차 공유 · 기본 | R26, community-skills | authoring/export · P4/P10 | SKILL.md+최소 sidecar·합성 예제·라이선스, typed route 재구성·private trace/credential/proof 반출0·명시적 Git publish |
| F68 | 제안/할 수 있는 일의 Git 스킬 · 기본 | R27, 기존 work-cards/agent UI | React cards/catalog · P4/P7 | local ranking·ready/install/connect/unsupported·숨김/mute·설치만으로 외부 실행0·기존 채팅 중심 흐름 |
| F69 | 볼트 연결·credential 자동 재사용 · v1 선택 설치 | R28, credential-use | trusted-device broker/typed adapter · P0/P6/P8 | 개인 binding 자동 선택·범위/목적지/수신인 검사·model secret0·잠금 대기/재개·기기 local grant·online dispatch commitment/revoke 순서·same effect 복구 |
| F70 | 공식·커스텀 데이터 연결과 추적 · 기본 | R29, 기존 source_protocol/source_adapters의 공통 계약 | core/store/source connectors/React · P3/P6 | connection/scope/epoch·원문/version/batch/run provenance, ingest 전용 token·revoke, heartbeat/commit/coverage 구분, official≠ready, custom 보고≠provider 증명, DATA01~DATA03/DATA06~DATA08 |

## 사용자 경험에서 제외하지 않을 세부 조건

전송 후 사진은 말풍선에 남고 보관함에서도 바로 확인해야 한다. 전송 실패가 입력창을 덮어쓰면 안 된다. 업로드와 녹음 처리 때문에 다른 메시지를 막으면 안 된다. 기술적인 자료 건수나 내부 승인 단계를 기본 채팅에 반복 표시하지 않는다. 메모리 관리는 검색→항목→수정/망각 후 목록으로 돌아오는 흐름으로 구현한다. 삭제는 대화/기억/문서/원문/볼트의 실제 범위를 설명하되 매 조회에 확인을 요구하지 않는다.

## 현재 미지원 기능을 지원으로 오해시키지 않을 것

Kakao 발송·전화 발신·Slack 쓰기·reply-all·첨부 메일 발송·미래 생성 본문의 자동 발송·자동 구매/웹 예약·임의 shell/code 설치는 현재 기본 계약에 없다. 새 서비스 요청을 재사용 절차로 기록하는 것과 실제 adapter 구현은 다르다. 공식 adapter는 Rust 코드·합성 fixture·계약 시험·버전 배포로 추가한다. 커스텀 외부 수집기는 [공통 HTTP 수집 계약](data-connections.md)으로 연결하고 서버가 그 코드를 실행하지 않는다. 카톡 수집기는 R29에 따라 공개 구현·패키징 대상에서 제외한다. 스킬은 이를 자동으로 만들어진 연결처럼 표시하지 않는다.

## 시험 선정 규칙

원본에서 정적으로 확인한 Python test 함수772개와 TypeScript test 선언215개는 전체 source 표면의 정적 수다. parametrize·dynamic tests·skip·과거 릴리스 실행 수와 같지 않다. 기존 backend1,121·UI150을 새 Rust 통과 수로 쓰지 않는다. 파일별 test 목록은 `audit/test-surfaces.tsv`에 역사적 근거로만 둔다. 기존112 test/support 파일은108 reference·4 exclude이며 자동 이식 대상은0개다. 새 검사는 [평가 계획](testing.md)과 [시나리오](../audit/evaluation-scenarios.tsv)의 사용자 결과·실제 결함·독립 oracle로 선정한다. 구현을 따라 쓴 검사·동일 CRUD 중복·고정 prompt 문구·폐기한 framework 검사는 버린다. F ID 연결은 누락 탐지이며 테스트 개수나 품질 증명이 아니다.

### 이미지 첨부 구현 범위 (F10–F12/F14)

현재 이미지 단위는 PNG/JPEG/WebP 원본32MiB·48MP를 브라우저에서2400px·2MiB 이하로 준비하고, 서버가 서명/실제 decode/할당 한도를 다시 검사하여 EXIF 없는 JPEG와 실제 thumbnail을 저장한다. 파일별 진행·취소·같은 UUID/bytes 재시도가 독립적이며, 글을 보내도 미완료 업로드는 남는다. 준비된 최대4장의 ID/revision만 turn과 retry에 고정한다. 인증 no-store preview·확대·download를 composer/말풍선/보관함에서 제공한다.

이미지 연결은 provider revision/model/effort별 임의4×4 색상 probe를 실제 통과해야 한다. 이름으로 vision을 추정하지 않는다. 미지원 조합은 입력/선택을 보존하고 관리자 설정의 이미지 연결 확인으로 안내한다. 현재 turn의 이미지만 전송하며, 이전 이미지에서 파생된 대화는 직접 새 이미지를 본 것과 구분한다. 이미지 및 이를 상속한 대화는 모든 tool/메모리/기록 쓰기를 비활성화한다. 이미지 유래 기록 저장, 일반 파일 텍스트/PDF/STT/오디오/동영상/공유는 후속 단위이며 업로드 완료를 해당 처리 완료로 표시하지 않는다.

삭제는 typed destructive journal의 tombstone과 의존 답변·checkpoint·evidence 철회를 함께 처리한다. 처리 중/응답 중 늦은 완료는 복구하지 못하며, restart/restore의 미완료 처리는 재선택 필요 상태로 격리된다. 합성 HTTP→픽셀을 읽는 loopback provider→retry/reload/library 및 소유권·tombstone·object URL 수명 검증은 `crates/moosuri-server/tests/media.rs`, `store::media::tests`, `web/src/Media.test.tsx`에 있다.


F17의 기기 등록·일반 payload·계정/멀티탭 수명·소식함 연결과 F32의 quiet hours
푸시 억제를 schema23에 구현했다. [Web Push](web-push.md) 및
[검증 기록](reviews/2026-11-13-web-push.md)에서 실제 합성/브라우저 범위를 확인한다.
실제 휴대폰 수신 검증과 topic mute/전체 제안 정책은 별도 미완료 항목이다.

F27의 일회성 명시 요청·계획·읽기·영수증 검증 범위는 [맡긴 일](missions.md)에 설명한다. 반복 실행·대화 자동 추출·범위 확장은 아직 완료가 아니다.

F37의 현재 기본 캘린더 읽기·직접 양식 쓰기 범위는 [내 Google Calendar](calendar.md)에 설명한다. 일반 대화/agent Calendar 실행·반복/참석자 일정 변경은 포함하지 않는다.
