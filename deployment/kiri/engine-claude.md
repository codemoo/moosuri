# Kiri 엔진의 Claude Code 연결

Kiri 엔진은 등록한 회원의 요청을 호스트의 Linux 비루트 작업자에게 전달한다. 한 작업자에 한 회원의 런타임 튜플과 그 회원이 직접 인증한 Claude 계정을 등록한다. 회원 컨테이너는 기존 `/model/model.sock`을 사용한다. 공식 Claude Code가 자신의 인증 환경을 관리한다. [엔진 연결](subscriptions.md) · [Claude Code 이용 조건](https://code.claude.com/docs/en/legal-and-compliance)

## 관리자 화면에서 연결

공개 소스 개발판에서는 **관리 → 어시스턴트 엔진 → Claude 연결 → 내 Claude
구독 사용**으로 시작한다. 본인의 계정 이메일과 호스팅 동의를 입력하면 필요한
전용 실행 환경을 자동 준비하고, 같은 Kiri 화면 안에서 공식 Claude 로그인을
연다. 별도 터미널이나 외부 창에서 인증 주소·코드를 복사하지 않는다. 인증과
계정·모델 확인이 끝나야 연결 완료로 표시한다. 설치 API 키를 쓰려면 같은
화면에서 **API 키 사용**을 선택한다. [관리자 사용 안내](admin.md#설치-모델과-추론-강도)

로그인을 시작한 세션만 화면과 입력을 사용할 수 있다. 같은 세션의 새로고침은
진행 중인 요청을 다시 열며, 취소·로그아웃은 해당 브라우저와 로그인 프로세스를
정리한다. 서버 재시작이나 불확실한 정리는 화면의 복구 절차를 따른다.

Linux 설치 후보에서 자동 준비, 공식 로그인 화면, 모바일·데스크톱 조작,
세션 분리와 취소·복구를 검증했다. 실제 Claude 계정 인증·추론, 인증 상태의
재부팅과 일반 회원의 개별 구독 등록은 아직 미완료다. 운영 반영 여부는
[현재 상태](../../docs/status.md), 여정별 증거는
[Kiri 인증 검증](../../docs/reviews/2026-11-27-claude-kiri-auth.md)을 따른다.

## 운영자 참고: 내부 로그인 전달

개발판의 비공개 `claude-engine-login-bridge --config …`는 화면 연동용 자식
프로세스다. 비루트 작업자의 기존 비공개 설정을 읽고, 부모의 파이프로 공식 인증
주소와 고정 결과만 전달한다. 부모는 코드를 한 번만 보내고 결과까지 입력 파이프를
유지해야 한다. 종료 시 결과가 없으면 실패로 처리하며, 표준 오류는 버리고 인증
주소·코드를 기록하지 않는다. 이 명령 자체는 모델 연결을 등록하지 않는다.
상위 관리자 흐름이 준비·브라우저·검증·등록을 관리한다. 아래 명령과 JSON은
구버전 구성과 장애 조사용 참고이며 일반 사용자의 연결 절차가 아니다.
[초기 전달 경로 검증](../../docs/reviews/2026-11-27-claude-subscription-bridge.md)

## 운영자 참고: 수동 구성

기존 호스트의 의존성을 보완할 때는 패키지 이름뿐 아니라 실제 바이너리와
제공 패키지를 확인한다. Docker CE의 `docker-ce-rootless-extras`가 이미
`rootlesskit`을 제공할 수 있다. 배포판의 동명 패키지를 무조건 설치하면 기존
Docker 패키지가 제거될 수 있으므로 패키지 관리자의 변경 예고를 먼저 확인한다.
누락된 `fuse-overlayfs` 등의 도구만 기존 엔진을 유지하는 범위에서 추가한다.
새 설치의 지원 기준과 절차는 [소스 설치 안내](source-install.md)를 따른다.

1. 전용 비루트 Linux UID에서 rootless Docker와 격리 프로필을 준비한다. 공식 Claude Code 바이너리, Moosuri 실행 파일, 이미지 다이제스트를 고정하고 배포 환경의 격리 수락 시험을 통과한 프로필을 사용한다. `auth_root`와 `scratch_root`는 작업자가 소유한 `0700` 디렉터리다. 프로필 JSON과 계정 이메일 파일은 작업자가 소유한 비공개 디렉터리에 `0600`으로 둔다. 프로필 필드와 시험 절차는 기존 실행 가이드를 따른다. [프로필과 격리 시험](../../docs/claude-execution.md#operator-profile)
2. 회원마다 고유한 UUID를 하나 만들어 `environment_id`로 지정한다. `expected_email_file`에 해당 회원의 Claude 계정 이메일을 공식 계정 화면에 표시된 대로 한 줄로 저장한다. 동일한 환경을 여러 회원에게 배정하지 않는다.
3. 같은 비루트 UID의 비공개 대화형 터미널에서 아래 명령을 실행한다. 사용자가 공식 Claude 로그인에서 자기 계정으로 인증한다. 성공 결과에 `profile_digest`, `environment_id`, `models`와 모델별 `effort_metadata`가 표시된다. 지원 강도를 확인할 수 없는 모델은 기본 강도로 사용한다.

   ```sh
   moosuri claude-engine-login \
     --profile-file /srv/moosuri-claude/private/profile.json \
     --environment-id "$CLAUDE_ENVIRONMENT_ID" \
     --expected-email-file /srv/moosuri-claude/private/owner-email
   ```

4. 작업자 설정의 `owner`에 이 회원의 계정·테넌트·볼륨·세대를 등록한다. `model`에는 위 `models`에서 고른 구체적인 ID를 사용한다. 엔진과 작업자에 같은 모델과 연결 리비전을 설정한다. 작업자 설정의 `adapter`는 아래 필드를 사용한다.

   ```json
   {
     "type": "claude_code",
     "settings": {
       "profile_file": "/srv/moosuri-claude/private/profile.json",
       "profile_digest": "<로그인 명령이 출력한 다이제스트>",
       "environment_id": "<회원 전용 환경 UUID>",
       "expected_email_file": "/srv/moosuri-claude/private/owner-email"
     }
   }
   ```

작업자는 시작할 때 선택한 환경의 이전 컨테이너를 정리하고, 공식 초기화 응답에서 구독과 이메일을 확인한다. 각 사용자 요청에서도 같은 Claude Code 프로세스의 초기화 응답으로 이메일·구독·모델을 확인한 뒤 사용자 메시지를 전달한다. 모델 목록은 공식 초기화에서 확인한 기본 구독 모델을 제공한다. 다른 Claude 계정으로 전환할 때는 새 환경 UUID와 연결 리비전을 설정한다. [Claude 모델 탐색](../../crates/moosuri-connectors/src/claude_code/discovery.rs) · [Claude 실행 환경](../../crates/moosuri-connectors/src/claude_code/sandbox.rs)

추가 모델·강도는 브로커 설정과 워커 설정 양쪽의 `models`에 정확한 쌍으로 등록한다. 해당 계정에서 CLI가 허용한 조합을 사용한다.

작업자는 엔진의 출력 토큰 한도를 `CLAUDE_CODE_MAX_OUTPUT_TOKENS`로 전달한다. Claude Code는 이 값을 대부분의 요청에 적용하며 모델별 한도를 적용할 수 있다. 엔진은 CLI가 반환한 사용량을 원래 호출 원장에 기록한다. [Claude Code 환경 변수](https://code.claude.com/docs/en/env-vars#available-environment-variables)
