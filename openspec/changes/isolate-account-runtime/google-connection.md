# 회원 Google 연결

Gmail, Drive, Docs, Slides, Sheets의 연결 버튼은 회원 Kiri에서 새 Google 세션을 연다. 볼트는 등록된 회원의 이메일과 accounts.google.com에 해당하는 로그인 항목을 찾는다. 항목이 없으면 볼트 안에서 비밀번호를 받아 암호화 저장하고 같은 요청을 재개한다. 항목이 여러 개면 회원이 선택한다. 서비스 연결에는 해당 서비스의 OAuth 동의와 서버에서 확인한 계정이 필요하다.

## 실행 경계

- 게이트웨이는 현재 세션, 회원, 설치, 볼륨, 세대와 권한 epoch를 검사한다. 서버가 생성한 일회용 식별자에 비밀번호 암호문을 묶는다.
- 볼트는 자체 서버에서 확인한 회원 Kiri 공개 키로 선택 항목을 암호화한다. Kiri는 새 Chrome 프로필의 Google 비밀번호 양식에 표시된 계정이 일치할 때만 비밀번호를 사용한다. 로그인 완료는 Google 계정 홈의 계정 표시로 확인한다.
- OAuth 콜백을 받은 게이트웨이는 기존 PKCE 흐름과 회원별 권한을 검사한다. 시작한 회원 세션에서 서비스 연결 영수증을 확인한다. Google의 추가 확인 요청, 차단, 설치 앱 미등록은 각각 별도 결과로 표시한다.
- Linux 운영 설치는 볼트에 등록된 회원 한 명과 전용 Kiri VM을 사용한다. 게스트의 기존 kiri-chrome 도우미 옆에 google_connection.py와 enroll_google_connection.py를 설치한다. 등록 도구의 공개 출력을 볼트 서비스의 --kiri-recipient-file에 지정한다. 수신 소켓은 root 전용이다. 소켓 서비스의 사용자와 SSH 실행 파일은 해당 VM 운영 계정에 맞춘다. SSH 실행 파일이 systemctl --user를 호출하는 설치는 서비스 환경의 XDG_RUNTIME_DIR와 DBUS_SESSION_BUS_ADDRESS를 해당 사용자의 실제 Linux UID에 맞춘다.

## 검증

Linux 브라우저 시나리오는 vault-web/tests/browser-google-connection.py, 연결 화면 시나리오는 web/src/kiri-google-connection.test.ts, Kiri 로그인 및 실패 경계는 deployment/kiri/test_google_connection.py에서 관리한다. 실제 운영 로그인과 GWS 동의는 각 단계의 공급자 결과를 기록한다.
