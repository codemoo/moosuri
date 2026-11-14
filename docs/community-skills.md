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

source catalog는 Git에서 관리하고 `moosuri skills catalog`가 선택한 exact public commit+package digest를 가진 static index와 패키지를 만든다. index 자신의 commit을 같은 commit 안에 자기 참조로 넣지 않는다. 생성 index는 release artifact다. 서비스는 설정된 catalog source만 자동 갱신하고 사용자가 고른 패키지만 받는다. 기여자 GitHub PR merge가 사용자 설치/업데이트를 자동 수행하지 않는다. 실제 repo 공개·release publish는 이번 문서 작업에서 수행하지 않는다.

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

현재 설치 경로는 정확히 네 파일을 받는 앱 내장 목록과 아래의 공개 GitHub 원본/설정된 정적 목록이다. references/assets·archive·plain SKILL import는 아직 지원하지 않는다. 실제 작성 template은 [community](../community/README.md)에 둔다. 외부 표준에서 가져온 파일의 redistribution 권리와 버전을 확인한다.

## 개인별 설치와 수명

package identity는 정규화한 repository+subdirectory이고 표시 이름은 별도다. 이름 충돌로 다른 제작자의 skill을 덮어쓰지 않는다. install 시 branch/tag를 full commit으로 resolve하고 bounded archive의 content digest를 기록한다. HTTP redirects·GitHub source hosts·archive 경로/압축 크기/파일 수/symlink를 검사한다. Git hook/submodule/install script는 실행하지 않는다. digest는 byte 동일성이지 안전성 인증이 아니다.

공개 immutable package bytes는 서버에서 공유 cache할 수 있지만 `user_id, source, pinned_revision, enabled, local_edit, policy_version`은 개인별 SQLite 상태다. 원문/학습 proof/credential/실행 내역은 공유하지 않는다. SQLite가 설치 상태의 정본이며 portable `skills.lock.json`은 export/import 산출물이다. 별도 lockfile과 DB를 양쪽 정본으로 만들지 않는다.

업데이트는 다운로드/검증/staging→원자적 active pointer 교체다. 실패하면 이전판 유지, 이전판으로 rollback 가능, local 수정은 덮어쓰지 않고 fork/수정본 유지로 처리한다. 새 요청만 새 버전을 쓰며 진행 run은 원래 revision을 고정한다. 위험한 권한/새 연결이 필요한 변경은 필요한 연결 한 번을 안내하고 자동 확장하지 않는다. 기본은 업데이트 알림, 사용자가 “모두 업데이트”하면 현재 허용 범위 내에서 일괄 수행한다.

disable/remove는 새 사용을 막고 종속 예약을 needs_skill로 멈춘다. 진행 중 외부 effect/receipt는 지우거나 재발송하지 않는다. 사용자 삭제/철회는 run generation을 올리고 늦은 발행을 막되 실제 완료한 영수증은 보존한다. 한 사용자의 삭제가 다른 사용자의 설치본을 없애지 않게 reference count/GC를 사용한다. curated catalog에서 철회된 악성/손상 revision은 새 사용을 차단하고 이유를 표시하며 자동으로 다른 제작자 패키지로 바꾸지 않는다.

설치 관리용 예정 CLI는 `moosuri skills install <github-url>`, `list`, `update [id]`, `rollback <id>`, `disable <id>`, `remove <id>`다. 로컬 파일 작성·검사는 아래의 별도 native 명령으로 지원한다. 일반 사용자는 UI 카드만으로 같은 기능을 수행한다. CLI는 server 인증한 user scope를 쓰고 arbitrary `--user`로 타인 설치를 바꾸지 않는다. 아직 실행 가능한 명령으로 문서화하지 않는다.

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

예약 needs_skill·수정본 fork·portable lockfile·개인 학습 재구성/export는 후속이다. 명시적으로 설정한 정적 catalog 발견/갱신은 아래 runtime 단위이며 자동 설치/업데이트는 포함하지 않는다.

같은 이름의 bundled 절차는 최초 managed 설치 시 단일 community pin으로 대체한다. disable/remove tombstone은 이후 새 run에서 bundled twin까지 discovery에서 제외하며 native 도구 권한 자체는 바꾸지 않는다. 다시 추가하면 복구한다. 버전이 바뀌어도 tombstone/숨김은 source+subdirectory identity로 유지한다.

## 공개 GitHub 원본 직접 추가

제안 화면의 **GitHub 원본에서 추가**는 공개 저장소 root URL·패키지 디렉터리·브랜치/태그/전체 commit을 받는다. `https://github.com/author/repository`와 `community/skills/example`처럼 입력한다. **원본 확인**은 공개 원본을 읽고 exact commit·version·license·필요 연결을 보여주며 설치하지 않는다. **추가** 또는 **이 버전으로 업데이트** 한 번이 그 SHA-256 snapshot을 현재 사용자에게 활성화한다. 작업·메시지·예약은 실행하지 않는다. Gmail 최종 발송 승인은 기존 경계를 따른다. 다른 버전 확인은 사용자가 다시 원본을 지정하는 수동 흐름이며 자동 upstream 갱신은 없다.

Git subprocess·archive·redirect 대신 고정 `api.github.com`의 commit/tree/blob를 읽는다. ref는 한 번만 resolve하고 이후 full immutable object만 읽는다. private 인증·cookie·proxy·개인 기억/대화는 보내지 않는다. 일반 read-only 공개 API이므로 GitHub 요청 제한·오프라인이면 실패를 그대로 표시하고 이전 활성판을 유지한다. 소스 URL만 붙이는 대신 root URL/상대 경로/ref 세 항목을 받으며 slash가 포함된 ref도 모호하게 추측하지 않는다.

현재는 정확히 SKILL.md/moosuri.toml/examples.json/LICENSE 네 regular100644 파일과 기존 instruction 형식1/Apache-2.0/allowlisted capability만 지원한다. symlink/submodule/executable·scripts/hooks·references/assets·다른 파일은 거절한다. 파일당32KiB/전체64KiB, 디렉터리6단계, 최대12요청/30초/응답합계2MiB 등 bounded tree/blob 검사를 적용한다. Git blob SHA-1은 길이로 구분한 Git object identity와 bytes 일관성을 확인하고, 앱 snapshot은 별도 SHA-256을 쓴다. 어느 해시도 안전성·license 진실성·실행 성공을 보증하지 않는다.

외부 envelope2와 stable opaque ID는 정규화한 repository+directory를 포함한다. 같은 표시 이름을 고른 다른 제작자나 다른 하위 경로가 서로 덮어쓰지 않는다. 외부 원본은 bundled 절차를 대체하지 않는다. 기존 curated envelope/hash/pin은 재작성하지 않으며, 이미 admit된 run은 원래 package bytes·metadata·tool profile을 유지한다. 다운로드는 SQLite writer 밖에서 수행하고 download 뒤와 activation 때 각각 currentUserContext를 다시 검증한다. 새 schema migration은 없다. 사용자당64개 외부 identity(tombstone 포함)까지만 보관하며 공개 staging cache의 GC는 후속이다.

private GitHub 인증·범용 SKILL import·archive·스크립트·자동 설치판 갱신/정적 release 게시·개인 학습 export·cache GC는 아직 지원하지 않는다.


## 로컬 작성 CLI

`moosuri skills init <slug> [--output DIRECTORY]`는 새 디렉터리에 SKILL.md/moosuri.toml/examples.json/LICENSE 네 파일을 만든다. 기본 경로는 현재 위치 아래 `<slug>`이며 지정한 parent는 이미 있어야 한다. slug는 소문자 ASCII·숫자·하이픈1–64자다. 원래 합성 자료 요약 절차, 정상/지원하지 않는 요청 예제와 표준 Apache-2.0 전문을 제공하며 기존 개인 자료·학습 이력·설치본은 읽지 않는다.

`moosuri skills check DIRECTORY`는 installer와 같은 content validator로 호환성을 확인한다. LF `---` frontmatter의 name/description, 비어 있지 않은 본문, format_version1·instructions·숫자 x.y.z version·Apache-2.0 sidecar와 LICENSE 표식, 합성 provenance와2–8개 정상/실패 예제를 검사한다. 파일당32KiB/합계64KiB, 정확히 네 UTF-8 regular nonexecutable single-link 파일만 허용하며 symlink·special file·추가 파일을 거절한다. 패키지 이름은 디렉터리명이 아니라 SKILL.md의 name이며 GitHub installer와 같은 matching-name 검사를 거친다.

현재 capability 이름은 `knowledge_search`, `knowledge_read_source`, `knowledge_read_memory`, `decimal_calculate`, `mail_search`, `mail_read`, `mail_thread`, `mail_prepare_draft`, `web_read`다. sidecar는 기존 계약의 single-line JSON scalar/array 부분 문법을 쓰며 unknown key·capability는 거절한다. 확인 결과는 파일 형식·호환성일 뿐 안전성·저작권·작업 성공 인증이 아니다. 진단은 파일/규칙을 설명하고 원문·credential을 출력하지 않는다.

init/check와 catalog는 data-dir 해석과 Store open 전에 실행하므로 설치·계정·서버가 필요 없다. 저장소 안의 일반 작성 경로는 허용하지만 `..`·symlink 경로·app-data marker가 있는 경로/하위 경로는 거절하며 다른 tree의 권한을 바꾸지 않는다. 기존 ancestor는 모두 root 또는 현재 effective UID 소유여야 하고 group/world writable이면 sticky여야 한다. root/current-user 소유 sticky temp 아래에서도 각 child 소유권을 계속 확인하며 다른 UID 소유 sticky 디렉터리는 허용하지 않는다. pinned ancestor descriptor chain을 유지하고 게시·실패 정리 전에 다시 검사한다. root/같은 UID의 동시 경로·권한 변경은 신뢰한 운영자 경계다. macOS는 deny-only ACL만 지원하고 grant ACL은 거절한다. Linux는 extended/default/NFSv4 ACL이 검출되면 지원하지 않는 경로로 거절하며 ACL 조회 실패도 진단한다. 알려진 로컬 Unix 소유권·mode/ACL 정책 밖의 filesystem 권한을 포괄적으로 인증하지 않는다. init은 새 sibling staging에 네 파일을 완성·sync한 뒤 Linux/macOS no-replace rename으로 게시하고 parent를 sync한다. 기존 목적지는 덮어쓰지 않는다. 실패 정리는 만든 inode가 같은 파일만 제거하며 모르는 내용은 보존한다. 게시 후 parent sync 오류에서는 완성된 목적지가 이미 있을 수 있으므로 검사한다.

작성자는 파일을 직접 편집·검토하고 원하는 commit/PR/push를 수행한다. 이 CLI는 Git subprocess·network·설치·실행·자동 게시를 하지 않는다. 이후 사용자는 기존 GitHub UI에서 exact commit snapshot을 선택하며 기존 도구 권한은 늘지 않는다. private-learning export·설치 snapshot 직접 수정·GC는 계속 후속이다. 정적 산출물 생성은 아래 별도 명령을 따른다. [작성 안내](../community/README.md)와 [합성 검증 기록](reviews/2026-11-13-community-authoring.md)을 따른다.


## 정적 catalog 산출물 생성

```sh
moosuri skills catalog --repository https://github.com/codemoo/moosuri \
  --commit 7f91c0ad8ef38049c5e0e601658da9b99b5f824e \
  --package community/skills/source-briefing \
  --package community/skills/evidence-comparison \
  --output ./public-release
```

기여자는 네 파일을 check하고 평소 Git 검토·commit·PR·push를 마친 뒤 실제 공개된40자리 소문자 commit과 명시한1–16개 디렉터리를 선택한다. branch/tag·로컬 파일·사용자가 입력한 SHA에 로컬 bytes를 붙이는 경로는 없다. 고정 공개 GitHub adapter가 commit/tree/blob를 확인하고 요청 commit과 응답 commit이 같을 때만 생성한다. 미커밋 변경은 읽지 않는다. 잘못된 commit·추가 파일·symlink/executable·변조 blob·지원하지 않는 license/capability는 기존 취득/installer 검증에서 실패한다. 인터넷이 필요하며 private 인증·proxy·cookie·개인 설치/학습/credential 접근은 없다.

결과는 새 디렉터리의 `catalog.json`과 `package-<content_digest>.json`이다. 선택 디렉터리 순으로 정렬하며 시간·로컬 경로·index 자체의 commit은 넣지 않아 같은 공개 bytes/선택이면 동일하다. package는 기존 external envelope2와 동일하며 source repository+directory+commit+이름+네 파일의 길이 구분 SHA-256이 `content_digest`다. `artifact_sha256`은 JSON 파일 bytes의 별도 SHA-256이다. 예를 들어 `shasum -a 256 public-release/package-*.json` 결과를 index의 해당 `artifact_sha256`과 비교하고, source_commit·선택 디렉터리·license/capability·파일을 검토한다. 해시는 안전성·권리·진위 서명이나 실행 성공 증명이 아니다. 여섯 curated envelope1/pin과 `community/catalog-pins.json`은 재작성하지 않는다.

패키지별 기존12요청/2MiB/30초·파일32KiB/합계64KiB, 전체16패키지/120초/8MiB 산출물 제한을 적용한다. 모든 취득·검증이 끝난 뒤 init과 같은 trusted ancestor descriptor/no-replace 원자 게시로 완성 디렉터리를 만든다. 기존 init/check도 같은 소유권·writable/sticky·지원 ACL 경로 경계를 적용한다. 실패 시 기존 목적지와 활성 설치본은 유지하며 다운로드 중에는 output을 만들지 않는다. 게시 후 parent sync 실패에서는 완성 목적지가 이미 있을 수 있어 검사한다. 진단은 고정된 규칙·상태만 표시한다.

이 명령은 로컬 release 파일 생성까지만 한다. 사용자가 검토한 파일을 배포하는 GitHub release/job·자동 게시는 없다. 서버 catalog 소비는 아래 별도 설정 단위이며, 기존 GitHub 원본 UI에서도 같은 repository/directory/exact commit을 확인하고 추가할 수 있다. 생성 index를 앱 data-dir에 복사하거나 설치 snapshot/내장 pin을 고쳐 활성화하지 않는다. 개인 학습 재구성/export는 별도 미구현 범위다.

## 설정된 정적 catalog를 제안 카드에서 읽기

서버 운영자가 **이미 공개한** catalog 산출물의 GitHub repository/ref/directory를 지정하면 기존 제안 카드가 그 목록을 읽는다. 기본값은 미설정이며 공개 branch/release가 존재한다고 가정하지 않는다. 예를 들어 기여자가 catalog 명령으로 만든 파일을 같은 공개 repository의 별도 `catalog-release` branch 아래 `public-release`에 검토·게시한 뒤에만 다음 설정을 쓸 수 있다.

```sh
moosuri serve --origin https://assistant.example.com \
  --community-catalog-repository https://github.com/codemoo/moosuri \
  --community-catalog-ref catalog-release \
  --community-catalog-directory public-release
```

ref는 단순 branch/tag 이름 또는 full commit이며 slash ref는 현재 지원하지 않는다. 세 항목을 모두 지정해야 한다. 이 예는 게시/존재 보증이나 자동 release 작업이 아니다. 원본 source_commit과 artifact를 게시하는 ref는 다를 수 있어 index 자기 참조가 필요 없다. index.repository는 설정한 repository와 같아야 한다.

카드를 열 때 catalog.json만 기본 하루 한 번 조건부 ETag로 확인한다. “공개 목록 업데이트 확인”은 수동 확인이며 반복 요청도 최소1분 간격이다. 실행 scheduler에 주기 작업을 붙이지 않는다. private 대화/검색어/기억/연결 정보는 보내지 않고 별도 LLM 호출도 없다. 고정 raw.githubusercontent.com HTTPS만 쓰며 proxy/redirect/cookie/private 인증은 지원하지 않는다.

새 항목은 “내용과 원본 확인” 전에는 패키지 호환/실행 준비 완료로 표시하지 않는다. 선택한 artifact만 SHA-256으로 확인하고 기존 exact public GitHub commit/tree/blob adapter로 네 원본 파일을 별도로 취득·비교한 뒤 기존 v2 validator/개인별 설치에 넘긴다. index의 source_commit/digest 선언만으로 공개 원본 검증을 대체하지 않는다. 이미 그 exact 공개 snapshot을 검증해 cache한 경우 재사용할 수 있다. 원본 불일치·GitHub 제한·움직이는 ref의 파일 교체·손상·중단은 이전 설치판을 유지한다. 확인이나 목록 갱신은 설치/업데이트/실행/메일을 시작하지 않는다.

SQLite35에는 공개 index bytes/digest/ETag/시간/짧은 lease만 저장한다. 매 사용 전에 index를 다시 검사하며 offline에서는 마지막 검증 목록과 설치본을 유지하고 오래된 상태를 표시한다. cache 손상은 새 제안에서 제외하고 조건 없는 재취득을 요구한다. 설치판은 사용자가 명시적으로 검증·업데이트하기 전까지 그대로 쓰며 사용자별 hide/enable/version/tombstone과 기존 run pin/receipt 경계를 재사용한다. 목록에서 숨기기는 내용 다운로드 전에도 가능하고 버전이 바뀌어도 유지한다. hash는 기본 카드가 아닌 상세의 접힌 버전 정보에 있다.

설정 source만 자동 발견할 뿐 자동 설치/업데이트·코드/hooks/script·private GitHub·범용 import·개인 학습 export·cache GC·임의 publisher 서명 신뢰·미지원 native adapter는 추가하지 않는다. 다운로드 검증은 byte/source 일치이지 안전성·저작권·사용 성공 인증이 아니다.

새 버전을 미리봐도 기존 설치본의 시작 가능 상태를 유지하며, 첫 진입과 일반 새로고침에서도
목록 연결 상태를 갱신한다. 독립 검토의 두 결함을 실제 실패 검사로 재현·수정하고 Linux와
실제 Chrome에서 검증했다. [수락 범위와 제한](reviews/2026-11-14-community-catalog-runtime.md).
