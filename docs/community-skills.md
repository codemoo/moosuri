# GitHub 커뮤니티와 에이전트의 제안

기여자는 GitHub에서 함께 만들고, 사용자는 에이전트의 **제안·할 수 있는 일**에서 발견하고 추가한다. 별도 marketplace 서비스·계정·검색 서버를 처음부터 만들지 않는다. 기존 `web/app/work-cards.tsx`의 Suggestions/ProviderCards, `agent.tsx`의 제안 화면은 UX 참고이며 이전 코드/테스트를 그대로 이관하지 않는다. 기본 화면은 계속 채팅이다.

## 사용자 흐름

| 현재 상태 | 카드에 표시할 내용 | 주 동작 |
| --- | --- | --- |
| 설치됐고 필요한 연결/도구가 있음 | 할 수 있는 일·예상 결과·짧은 제안 이유 | “해줘” 또는 필요한 입력만 받기 |
| 미설치, compatible 연결 준비됨 | 설명·제작자·출처, 추가되는 기능 | “추가하고 시작” |
| 패키지는 compatible, 개인 연결/credential 없음 | “메일 연결 필요” 같은 실제 부족 조건 | “연결하고 시작” → 연결/항목 선택 후 원래 요청 이어가기 |
| native adapter/script 지원이 없음 | “현재 지원 안 함”, 필요한 기능 | 설명/소식 받기. 실행 완료/연결 완료로 꾸미지 않음 |
| 설치 후 업데이트 있음 | 일반 카드 동작은 유지, 상세에 새 버전 안내 | 업데이트·이전 버전·비활성화·삭제 |

같은 install/실행을 두 번 확인하는 화면은 만들지 않는다. “추가하고 시작”은 명확히 표시한 그 작업에 대한 사용자 동작이다. 단순 설치·추천 노출 자체는 이메일 발송/외부 변경/credential 연결 동의가 아니다. 실제로 부족한 수신인·계정·시각만 대화에서 묻는다. 이미 허용한 연결/볼트 binding은 [credential 사용](credential-use.md)에 따라 자동 사용한다.

상세 화면에서만 source URL·version·필요 capability·license·업데이트 내용을 연다. 기본 카드에 hash·도구 개수·내부 구현 설명을 쏟아내지 않는다. 제안 숨기기/나중에/주제 그만 받기는 user+stable package/topic identity로 저장해 버전이 바뀌어도 존중한다. 자동 제안은 적은 수로 묶고 실제 가능한 것부터 보여준다. 새로운 package 설치만으로 예약을 생성하거나 과거 요청을 자동 재실행하지 않는다.

공개 catalog를 내려받아 사용자 연결 상태·최근 작업의 로컬 tag·선호·mute로 서버 내부에서 후보를 고른다. catalog 검색에 private 대화/기억/검색어를 보내지 않는다. 기본 추천에는 별도 LLM 호출이 없다. 다운로드 URL에는 public package ID/version이 나타날 수 있지만 개인 문맥은 포함하지 않는다. 업데이트 체크는 cache/ETag와 느린 주기(기본 하루1회 또는 사용자 새로고침)로 하며5초 task scheduler에 붙이지 않는다. offline이면 마지막 검증 catalog와 설치본을 쓰고 상태를 정확히 표시한다.

## 기여·배포 경로

v1은 같은 `moosuri` repository의 `community/skills/<slug>/`를 기본 기여 위치로 한다. 작성자는 template→SKILL.md/호환 정보/합성 예제→PR 순서로 기여한다. 처음부터 별도 registry 운영팀·DB·배포 서버를 요구하지 않는다. public GitHub 링크를 직접 추가하는 고급 경로도 제공하되 curated 목록과 구분한다. private GitHub 인증은 후속이며 공개 공유의 선행 조건이 아니다.

source catalog는 Git에서 관리하고 release job이 exact commit+package digest를 가진 static index와 패키지를 만든다. index 자신의 commit을 같은 commit 안에 자기 참조로 넣지 않는다. 생성 index는 release artifact다. 서비스는 설정된 catalog source만 자동 갱신하고 사용자가 고른 패키지만 받는다. 기여자 GitHub PR merge가 사용자 설치/업데이트를 자동 수행하지 않는다. 실제 repo 공개·release publish는 이번 문서 작업에서 수행하지 않는다.

기본 기여는 small recipe로 충분하다. core 도구/권한/설치 형식 변경은 OpenSpec change와 Rust adapter PR, 기존 도구를 조합하는 ordinary skill 추가는 간단한 skill PR+예제로 처리한다. 모든 스킬마다 무거운 제안서4개를 요구하지 않는다. capability/schema가 바뀌는 경우만 새 계약을 논의한다.

## 작은 패키지와 호환성

표준 Agent Skills의 `SKILL.md` frontmatter(name/description)와 필요한 references/assets를 기본으로 쓴다. `scripts/`·hooks·dependency installer는 v1에서 실행하지 않는다. script가 필수인 외부 skill은 inspect 가능하지만 실행 미지원으로 표시한다. 파일 형식이 같다는 이유로 Claude/OpenClaw의 도구·계정·권한이 생기지 않는다. `allowed-tools`는 요청 정보일 뿐 moosuri의 실제 권한을 생성하지 않는다.

모두가 편하게 쓸 수 있는 native catalog 항목에는 작은 `moosuri.toml` sidecar를 제공한다. 필수 정보는 format version·skill version·설명/입력·기존 capability 이름과 호환 version·mode다. license는 SPDX 또는 포함한 LICENSE로 명확히 한다. typed route가 있을 때만 route를 추가하고 기존9tool/8step validator와 evidence 규칙을 재사용한다. plain SKILL.md는 instruction import로 받되 machine-readable 요구가 없으면 호환성 확인 필요 상태이며 바로 실행 가능한 native 기능으로 추천하지 않는다.

sidecar는 code/SQL/shell/새 endpoint/credential 값을 정의하지 않는다. 필요한 tool이 없다면 Rust connector를 별도 기여한다. 인증은 capability와 [credential binding](credential-use.md)의 handle을 통해서만 제공한다. package가 vault item ID·다른 사용자·임의 host를 골라 secret을 읽게 하지 않는다.

예정 source 구조:

```text
community/skills/<slug>/
  SKILL.md
  moosuri.toml          # native 추천 카드·compatibility·typed route 정보
  examples.json         # 최소 정상 예제 + 핵심 실패/경계 예제
  references/           # 필요한 경우만
  assets/               # 필요한 경우만, 자체/재배포 가능한 파일
  LICENSE
```

첫 구현은 이 중 정확히 네 파일만 받는 앱 내장 curated catalog installer다. references/assets·외부 Git/archive·plain SKILL import는 아직 지원하지 않는다. 실제 작성 template은 [community](../community/README.md)에 둔다. 외부 표준에서 가져온 파일의 redistribution 권리와 버전을 확인한다.

## 개인별 설치와 수명

package identity는 정규화한 repository+subdirectory이고 표시 이름은 별도다. 이름 충돌로 다른 제작자의 skill을 덮어쓰지 않는다. install 시 branch/tag를 full commit으로 resolve하고 bounded archive의 content digest를 기록한다. HTTP redirects·GitHub source hosts·archive 경로/압축 크기/파일 수/symlink를 검사한다. Git hook/submodule/install script는 실행하지 않는다. digest는 byte 동일성이지 안전성 인증이 아니다.

공개 immutable package bytes는 서버에서 공유 cache할 수 있지만 `user_id, source, pinned_revision, enabled, local_edit, policy_version`은 개인별 SQLite 상태다. 원문/학습 proof/credential/실행 내역은 공유하지 않는다. SQLite가 설치 상태의 정본이며 portable `skills.lock.json`은 export/import 산출물이다. 별도 lockfile과 DB를 양쪽 정본으로 만들지 않는다.

업데이트는 다운로드/검증/staging→원자적 active pointer 교체다. 실패하면 이전판 유지, 이전판으로 rollback 가능, local 수정은 덮어쓰지 않고 fork/수정본 유지로 처리한다. 새 요청만 새 버전을 쓰며 진행 run은 원래 revision을 고정한다. 위험한 권한/새 연결이 필요한 변경은 필요한 연결 한 번을 안내하고 자동 확장하지 않는다. 기본은 업데이트 알림, 사용자가 “모두 업데이트”하면 현재 허용 범위 내에서 일괄 수행한다.

disable/remove는 새 사용을 막고 종속 예약을 needs_skill로 멈춘다. 진행 중 외부 effect/receipt는 지우거나 재발송하지 않는다. 사용자 삭제/철회는 run generation을 올리고 늦은 발행을 막되 실제 완료한 영수증은 보존한다. 한 사용자의 삭제가 다른 사용자의 설치본을 없애지 않게 reference count/GC를 사용한다. curated catalog에서 철회된 악성/손상 revision은 새 사용을 차단하고 이유를 표시하며 자동으로 다른 제작자 패키지로 바꾸지 않는다.

예정 CLI는 `moosuri skills install <github-url>`, `list`, `check`, `update [id]`, `rollback <id>`, `disable <id>`, `remove <id>`다. 일반 사용자는 UI 카드만으로 같은 기능을 수행한다. CLI는 server 인증한 user scope를 쓰고 arbitrary `--user`로 타인 설치를 바꾸지 않는다. 아직 실행 가능한 명령으로 문서화하지 않는다.

## 개인 학습에서 공유용 스킬 만들기

개인 학습은 기본 private다. “공유용으로 만들기”는 허용한 typed route·input slot·일반 설명·합성 예제로 **새 패키지**를 만든다. 원래 trace를 복사한 뒤 문자열 redaction으로 안전하다고 판정하지 않는다. 메일/채팅/이름·운송장·계정/resource ID·경로·첨부·스크린샷·memory·credential·receipt payload를 넣지 않는다.

사용자가 export 미리보기를 확인하고 내려받아 GitHub PR로 올리는 경로부터 제공한다. 자동 push·다른 사람에게 메시지 보내기·private 자료 전송은 없다. 향후 GitHub publish 연결을 추가할 때도 명시적으로 publish를 선택한 그 산출물만 보낸다. source 작성자의 성공 이력은 설명 자료이며 수신자의 검증 proof가 아니다. imported route는 각 사용자 candidate에서 시작하고 실제 독립 성공2회의 동일 규칙을 따른다.

## 유지보수와 핵심 검사

새 community 기여는 schema/상대 참조/재배포 license·required capabilities·합성 정상/실패 예제를 검사한다. 실행법·예제·제한이 없으면 인기 수만으로 추천하지 않는다. UI에는 “호환 확인”, “출처 확인”, “검증된 사용”을 구별하고 포괄적인 안전 인증처럼 표시하지 않는다. 초기에는 downloads/telemetry leaderboard를 운영하지 않는다.

최소 검사는 하나의 lifecycle fixture(install→중단 update→정상 update→rollback→remove), 두 사용자 enable/version/credential 분리, archive/path/size 변조, permission 확대 금지, export private canary, 제안 카드→추가/연결/실행/숨김이다. 라벨 하나마다 test를 늘리지 않는다. [testing](testing.md)의 COMM 시나리오에 연결한다.

참고한 구조: [Agent Skills](https://agentskills.io/specification), [Vercel skills](https://github.com/vercel-labs/skills/blob/main/README.md), [ClawHub](https://docs.openclaw.ai/clawhub). 표준 형식·Git install·version pin·local edit 보호를 가져오고 각 도구의 runtime/marketplace 전체를 moosuri 의존성으로 넣지 않는다.

## 첫 curated 수직 구현 (2026-11-13)

`/app/skills`의 제안 카드와 인증된 `/api/v1/community-skills` API에서 사용자별 추가·업데이트·이전판·활성/비활성·삭제·숨김을 제공한다. 추가는 로컬 상태 변경이며 실행이나 외부 effect를 시작하지 않는다. 준비된 카드의 시작은 작성 중인 새 대화 초안을 유지한 채 절차 이름을 앞에 붙여 채팅 입력을 연다. 사용자가 필요한 실제 요청을 입력하고 전송한다. Gmail 부재와 서버 웹 읽기 부재를 개인 readiness로 표시한다. 별도 install 승인 화면은 없다.

현재 여섯 패키지는 공개 저장소 `7f91c0ad8ef38049c5e0e601658da9b99b5f824e`의 실제 네 파일과 `community/catalog-pins.json`에 고정했다. Rust는 경로 집합·파일당32KiB/전체64KiB·frontmatter·엄격한 sidecar 부분 문법·allowlist capability·SPDX/license·정상/실패 합성 예제를 검증한다. format1은 현재 typed capability 계약1을 뜻한다. repository/commit/slug/정렬한 파일명과 원문을 길이로 구분해 SHA-256을 기록한다. 해시는 안전성 인증이 아니다. 네 파일 밖의 scripts/hooks/assets와 지원하지 않는 key/capability는 거부한다. 외부 파일·archive를 받는 경로는 없다.

SQLite0011은 immutable 공개 snapshot과 사용자별 active/previous pointer, enabled/hidden/revision을 저장한다. snapshot staging과 pointer 변경은 같은 SQLite transaction으로 원자적이다. 활성판 변경은 새 run에만 적용되며 기존 run의 exact pin과 기존 tool profile은 유지한다. disable은 새 admission을 막는다. remove는 해당 사용자에게 이미 admit된 관련 run을 취소하고 agent 상태를 invalidate하며 durable retry 철회를 남긴다. 늦은 실행·출력은 기존 lease/state fence로 막고 외부 receipt나 완료한 effect는 삭제하지 않는다. 공개 snapshot은 진행 run의 identity를 위해 유지하며 GC는 아직 없다.

자동 catalog 다운로드/업데이트·정적 release 생성기·외부 Git 주소·예약 needs_skill·수정본 fork·portable lockfile·개인 학습 재구성/export는 후속이다. 이번 단위를 그 전체 구현으로 표시하지 않는다.

같은 이름의 bundled 절차는 최초 managed 설치 시 단일 community pin으로 대체한다. disable/remove tombstone은 이후 새 run에서 bundled twin까지 discovery에서 제외하며 native 도구 권한 자체는 바꾸지 않는다. 다시 추가하면 복구한다. 버전이 바뀌어도 tombstone/숨김은 source+subdirectory identity로 유지한다.

## 공개 GitHub 원본 직접 추가

제안 화면의 **GitHub 원본에서 추가**는 공개 저장소 root URL·패키지 디렉터리·브랜치/태그/전체 commit을 받는다. `https://github.com/author/repository`와 `community/skills/example`처럼 입력한다. **원본 확인**은 공개 원본을 읽고 exact commit·version·license·필요 연결을 보여주며 설치하지 않는다. **추가** 또는 **이 버전으로 업데이트** 한 번이 그 SHA-256 snapshot을 현재 사용자에게 활성화한다. 작업·메시지·예약은 실행하지 않는다. Gmail 최종 발송 승인은 기존 경계를 따른다. 다른 버전 확인은 사용자가 다시 원본을 지정하는 수동 흐름이며 자동 upstream 갱신은 없다.

Git subprocess·archive·redirect 대신 고정 `api.github.com`의 commit/tree/blob를 읽는다. ref는 한 번만 resolve하고 이후 full immutable object만 읽는다. private 인증·cookie·proxy·개인 기억/대화는 보내지 않는다. 일반 read-only 공개 API이므로 GitHub 요청 제한·오프라인이면 실패를 그대로 표시하고 이전 활성판을 유지한다. 소스 URL만 붙이는 대신 root URL/상대 경로/ref 세 항목을 받으며 slash가 포함된 ref도 모호하게 추측하지 않는다.

현재는 정확히 SKILL.md/moosuri.toml/examples.json/LICENSE 네 regular100644 파일과 기존 instruction 형식1/Apache-2.0/allowlisted capability만 지원한다. symlink/submodule/executable·scripts/hooks·references/assets·다른 파일은 거절한다. 파일당32KiB/전체64KiB, 디렉터리6단계, 최대12요청/30초/응답합계2MiB 등 bounded tree/blob 검사를 적용한다. Git blob SHA-1은 길이로 구분한 Git object identity와 bytes 일관성을 확인하고, 앱 snapshot은 별도 SHA-256을 쓴다. 어느 해시도 안전성·license 진실성·실행 성공을 보증하지 않는다.

외부 envelope2와 stable opaque ID는 정규화한 repository+directory를 포함한다. 같은 표시 이름을 고른 다른 제작자나 다른 하위 경로가 서로 덮어쓰지 않는다. 외부 원본은 bundled 절차를 대체하지 않는다. 기존 curated envelope/hash/pin은 재작성하지 않으며, 이미 admit된 run은 원래 package bytes·metadata·tool profile을 유지한다. 다운로드는 SQLite writer 밖에서 수행하고 download 뒤와 activation 때 각각 currentUserContext를 다시 검증한다. 새 schema migration은 없다. 사용자당64개 외부 identity(tombstone 포함)까지만 보관하며 공개 staging cache의 GC는 후속이다.

private GitHub 인증·범용 SKILL import·archive·스크립트·자동 갱신/정적 release 생성·개인 학습 export·cache GC는 아직 지원하지 않는다.
