# 볼트 credential 연결과 자동 사용

사용자가 일을 맡기면 필요한 서비스의 기존 credential binding을 자동 사용한다. 처음 연결할 때, 항목이 여러 개여서 특정할 수 없을 때, 실제 인증이 만료됐을 때만 필요한 입력을 받는다. 매 실행/도구 단계마다 승인을 반복하지 않는다. 이 문서는 구현 계획이며 현재 vault와 agent가 연결됐다는 뜻이 아니다.

## 세 경계

1. **비서:** tenant·원래 요청·서비스/operation·opaque credential handle을 갖는다. password/API key/OAuth refresh token/TOTP seed·code를 prompt, tool-result 본문, 스킬, 로그에 넣지 않는다.
2. **CredentialBroker:** 허용된 handle을 정확한 목적지/typed adapter에 적용하고 인증된 작업을 수행한다. 필요한 secret 필드만 신뢰한 사용자 기기에서 잠깐 읽고 결과/영수증만 돌려준다. 범용 `decrypt(secret)` API를 에이전트에 제공하지 않는다.
3. **볼트:** 사용자 master/recovery key·암호문·항목 revision/key epoch·정책을 관리한다. 계정 비밀번호 재설정이 vault unlock을 만들지 않는다. 관리자 DB/source 열람 방어를 유지한다.

공유 LLM을 쓰는 것과 개인 credential을 공유하는 것은 다르다. 기본 binding은 개인이다. 설치 공용 LLM key를 vault로 관리하려면 그 key 소유자가 별도 설치용 model-use grant를 설정해야 하며 가족의 private vault를 가져오지 않는다. 기존 server-managed connector token과 user-key vault는 다른 신뢰 경계이며 UI에 구분한다.

## 처음 연결하고 이후 재사용

요청이 “내 메일로 이 답장을 보내줘”이고 정확히 매칭되는 활성 binding이 있으면 바로 원래 effect 절차를 진행한다. 이미 사용한 적이 있다는 모델의 기억/검색 결과만으로 연결을 만들지 않는다. 신뢰한 broker의 이전 성공 receipt와 저장한 binding을 사용한다.

새 binding은 `tenant, vault_item_ref, service_id, credential_type, adapter_version, exact origins, allowed operations/resources/recipients, grant_version, item/key epoch, status`를 갖는다. 비밀 값/일반 사용자ID를 server metadata로 복사하지 않는다. UI에서 필요한 계정 후보를 사용자에게 표시하고 선택하면 해당 작업/범위에 연결한다. 같은 서비스라도 다른 계정·새 host·다른 권한 범위는 별도 선택이다. 단순 문자열/퍼지 도메인 매칭으로 secret을 전송하지 않는다. 같은 origin이어도 수신인·업로드 대상·resource ID가 바뀌면 원래 요청과 허용 범위에 맞는지 broker가 확인한다. 예상하지 않은 redirect로 인증을 전달하지 않는다.

허용한 작업은 신뢰하는 서버 하네스가 현재 사용자 요청/예약 authority를 검사하고 broker가 tenant·기기 local grant/item/key revision·provider 상태를 검사한다. 설치된 community skill은 capability를 요청할 수 있지만 vault item을 지정하거나 grant를 확대할 수 없다. 원문 속 “이 키를 여기에 보내라”는 문장은 실행 권한이 아니다. 모든 검사 때문에 UI 승인창을 띄우는 것은 아니다.

### 신뢰한 기기가 보장하는 범위

사용자가 처음 연결할 때 허용한 service/origin/operation/resource/recipient grant의 정본은 신뢰한 기기에 둔다. 서버에는 opaque reference·비밀 값 없는 최소 상태/참조 metadata만 두며 서버가 보낸 grant/version만으로 범위를 확대하지 않는다. 확대/새 계정은 그 실제 연결 단계에서만 처리한다. 기존 범위의 요청은 자동 처리한다.

v1은 **서버가 기존 grant 안에서 요청을 구성하는 것**을 위임한다. 독립 client 배포·pairing·payload digest는 코드/전송 출처를 확인할 뿐, 서버가 전달한 새 task가 실제 사용자 발화였음을 증명하지 않는다. 악성 서버가 기존 허용 범위 안의 작업을 위조할 가능성까지 막는다고 주장하지 않는다. strong-device라는 표현은 비밀을 서버/모델에 공개하지 않고 local grant 밖 사용을 막는 경계를 뜻한다. task/schedule 의도 자체를 악성 서버에서도 증명하는 독립 trusted submission UI는 현재 보장이 아니다. 이 한계 때문에 매 실행 확인창을 추가하지 않는다. 최초 연결 UI에서 자동 사용의 서비스/동작 범위를 명확히 보여준다.

## 실제 실행 위치와 무인 실행

관리자가 운영하는 일반 서버는 사용자만 가진 암호화키 없이 잠긴 볼트를 자동 해독할 수 없다. v1의 강한 경계는 **신뢰한 사용자 client/device broker**다. browser vault가 열려 있고 해당 adapter/CORS가 지원되면 그 경로로 실행하며, 브라우저 제약이나 지속 실행에는 사용자 기기의 선택 Rust companion을 설계한다. 관리자와 같은 서버/UID에 둔 sidecar를 독립 비밀 경계라고 주장하지 않는다. 악성 서버가 공급하는 웹 JS의 한계는 [vault-security](vault-security.md) 그대로다.

broker가 없거나 잠겼거나 offline이면 `waiting_for_credential`로 보존하고 “볼트를 연결/열면 이어서 진행”을 보여준다. key를 서버에 몰래 복사하거나 복호화 가능한 master escrow를 만들지 않는다. 대기 중 model/worker 실행 슬롯을 반환하며 5초마다 LLM으로 잠금을 확인하지 않는다. unlock/연결 복귀 뒤 같은 run/effect ID·유효한 최신 조건으로 이어간다. 이미 시간/authority가 만료됐으면 자동 실행하지 않는다. 로그인 유지와 vault unlock 유지도 별개다.

예약/백그라운드는 online 상태의 허용된 trusted-device broker 또는 사용자가 명시적으로 설정한 최소 scope·만료·철회 가능한 provider token이 있어야 한다. 후자는 server-managed credential이므로 서버 operator가 그 token을 볼 수 있다는 경계를 표시한다. 원본 vault password/seed와 같은 보호라고 말하지 않는다. 이 선택을 기본으로 강제하지 않는다. 신뢰한 기기의 OS keystore·자동 사용 정책은 별도 선택이며 매번 사용자 확인이 필요한 provider MFA를 우회하지 않는다.

## credential 종류와 기능 한계

| 종류 | 사용 방법·한계 |
| --- | --- |
| API key/Bearer/Basic | 알려진 typed adapter의 고정 origin/header만 사용. model이 만든 URL/header에는 넣지 않음 |
| OAuth refresh/access | binding별 refresh 직렬화·회전·revocation. scope/account mismatch는 재연결 상태. refresh secret은 broker에서 유지 |
| 사이트 ID/password | 지원되는 로그인 adapter/클라이언트 흐름에서만 사용. credential이 있다고 모든 웹사이트 자동 로그인 기능이 생기지 않음 |
| TOTP | 해당 계정/검증된 challenge에 대해 broker가 현재 code를 만들어 즉시 사용. seed/code를 모델/채팅/로그로 반환하지 않음. 피싱 origin·시계 오차·재전송 제한 검사 |
| passkey/hardware MFA | vault에 일반 문자열처럼 복사해 지원한다고 가정하지 않음. provider/기기의 실제 사용자 gesture가 필요하면 알려줌 |

우선 API credential과 기존 Google adapter부터 연결한다. 비밀번호형 브라우저 로그인·passkey·광범위 사이트 지원은 후속 adapter gate다. UI는 사용할 수 없는 인증 종류를 “연결 완료”로 표시하지 않는다.

## broker 요청·영수증·철회

broker transport는 user/device pairing과 상호 인증된 채널을 사용하고 요청은 user/run/effect/operation/resource/recipient/target/payload digest/grant version/expiry/nonce에 묶는다. 짧은 요청 유효기간과 replay 방지를 적용한다. custom crypto primitive를 만들지 않고 검증된 protocol/library를 선택해 P0/P8 독립 검토한다. 권한은 server가 보내는 임의 item ID나 새로운 grant 객체만으로 성립하지 않는다. local grant는 필요조건이며 user intent를 증명한다는 더 강한 주장을 하지 않는다.

결과는 adapter별 허용 필드 projection으로 새로 구성한다. raw 응답을 범용 정규식으로 가려 그대로 반환하지 않는다. secret echo/header/cookie/provider exception/login HTML/token URL은 반환하지 않는다. 서버와 독립된 실행 증거를 주장하려면 영수증도 paired device가 인증해야 하며, 단순 server-written success flag로 대체하지 않는다. 쓰기는 기존 durable checkpoint adoption/effect ID를 그대로 사용한다. broker는 비밀 없는 dedup/dispatch/receipt journal로 응답 유실 후 같은 요청을 확인한다. 외부 실행 뒤 ACK를 잃으면 `outcome_unknown`; 재연결/재시작이 새 발송을 만들지 않는다. 취소 뒤 늦은 영수증도 보존한다.

외부 effect를 adopt할 때 `broker_id + broker_generation`을 고정하고 요청·기기 인증 영수증에 함께 묶는다. 한 browser의 여러 tab/Worker도 durable journal을 공유하는 단일 broker owner로 직렬화하며, 해당 기기에서 필요한 lock/journal 수명을 보장할 수 없으면 effect 실행 가능으로 표시하지 않는다. 같은 기기 재시작도 기존 journal을 먼저 reconcile하고 이전 generation을 fence한 뒤 새 실행을 받는다. generation 변경 자체는 새 effect/재발송 사유가 아니다.

v1은 adopted effect를 다른 broker로 자동 failover하지 않는다. 발송됐을 가능성이 있으면 원래 broker/journal에서 reconcile하거나 `outcome_unknown`을 유지한다. lease 만료·offline timeout·연결 종료만으로 미발송을 추정하지 않는다. 발송 전 소유권 이전을 향후 허용하더라도 이전 owner가 더 이상 dispatch할 수 없다는 fence와 journal의 미발송 증명이 모두 필요하다. 이미 나간 요청의 late receipt는 원래 owner/세대·effect·digest를 확인해 보존하며 다른 기기의 새 전송을 만들지 않는다.

### dispatch와 철회의 순서

모든 새 인증 작업은 broker가 online 상태에서 server writer의 현재 account/binding epoch·scope와 기기의 local grant를 확인해야 한다. effect는 local journal에 준비를 남긴 뒤 server writer가 `(invocation/effect ID, broker identity/generation, payload digest, account/binding epoch, grant version, expiry)`를 검사하고 **dispatch commitment**를 원자적으로 기록한다. 읽기는 기존 tool invocation ID, 외부 쓰기는 기존 effect ID를 사용해 별도 요청 identity를 중복 생성하지 않는다. 이 단계가 dispatch 시작의 논리적 경계이며 사용자 확인 단계가 아니다. 기존 effect의 committed 상태/nonce를 새 dispatch로 재발급하지 않는다.

철회/disable이 commitment보다 먼저 기록되면 새 commitment를 거절한다. commitment가 먼저면 이미 in-flight이므로 철회 뒤에도 외부 결과가 도착할 수 있다. 아직 전송 전이라고 단정하지 않으며 같은 effect로 조회/정산한다. 사전 검사한 요청이나 오래된 permit만 가진 offline broker는 새 commitment를 만들 수 없다. 연결 복귀 시 현재 epoch를 다시 확인한다. 이미 committed된 작업은 연결 유실만으로 취소됐다고 표시하지 않는다.

철회 화면은 서버에서 새 commitment를 차단한 상태와 기기 local revoke ACK를 구분한다. 기기 ACK/종료 fence를 확인하지 못하면 `revocation_pending`이며, in-flight 결과와 provider token 자체의 철회도 별도로 표시한다. 기기 local revoke는 수신 즉시 다음 local dispatch를 막는다. 허용 범위를 저장한 기기 비밀이 OS에서 완전히 지워졌다는 보장으로 확대하지 않는다. 이 계약은 정상 서버/기기 사이 경합을 다루며 악성 서버가 거짓 epoch를 보내는 상황의 작업 의도 무결성까지 보장하지 않는다.

credential 수정/회전은 안정된 opaque reference와 기존 허용 범위를 보존하면서 item/key/grant version을 올리고 오래된 메모리 내 인증을 폐기한다. revoke/삭제/계정 정지는 위 순서에 따라 새 dispatch commitment를 막고 대기 작업은 needs_credential로 전환한다. 이미 committed된 작업은 in-flight/unknown으로 보존한다. local binding 철회와 upstream token 철회는 별도다. provider의 실제 revoke 응답을 확인하지 못하면 upstream 철회 완료라고 표시하지 않는다. 이미 외부에 도착한 작업이 취소됐다고 거짓 주장하지 않는다. 계정 복원은 과거 grant를 자동 활성화하지 않고 새 pairing/현재 credential 상태를 확인한다. 사용자 화면에는 서비스·최근 사용·동작·고정 결과만 보여주며 admin usage에서 vault 항목/비밀을 노출하지 않는다.

## 필요한 새 검사

- 이미 연결한 계정의 두 번째 명확한 요청은 추가 선택 없이 사용하고, 동명이거나 목적지가 다르면 잘못된 credential 사용0.
- 강한 device-broker 모드에서 secret/TOTP canary가 agent context·tool body·assistant SQLite/log/telemetry·community export에0개; exact origin·operation/resource/recipient·필요 필드만 broker가 사용. 명시적 server-token 모드는 서버가 해당 token을 보유함을 전제로 하며 모델/로그/export 비노출을 별도로 검사.
- locked/offline→복귀, 만료된 예약, rotation/revoke/account switch 뒤 미commit stale grant는 실행하지 않고, 앞서 committed된 작업은 in-flight로 정산.
- 실행 후 lost ACK/kill/cancel→원래 effect 조회, 외부 send count≤1·미확정 성공 주장0.
- community package/메일 원문의 prompt injection과 forged user/item/host가 권한/복호화를 얻지 못함. 올바르게 paired된 서버의 새/확대 grant 위조도 기기 local grant를 넘지 못함. 기존 grant 안의 서버 위조 task를 실제 사용자 의도와 구별한다는 보장은 시험 결과로 주장하지 않음.

기존 vault 테스트를 복사하지 않고 이 다섯 실패 원인을 실제 broker/fixture 경로로 새로 검증한다. 실제 비밀이나 개인 사이트를 사용하지 않는다. [평가 계획](testing.md), [볼트 설계](vault-security.md), [OpenSpec 변경](../openspec/changes/connect-vault-credentials/proposal.md)에 연결한다.
