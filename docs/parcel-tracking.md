# 일본우편 배송 조회

공개 HTTP 읽기가 켜진 새 대화에서 `일본우편 0000-0000-000 배송 조회해줘`처럼
배송사와 운송장을 함께 요청한다. 위 번호는 합성 예시이며 실제 배송물이 아니다.
현재 지원 배송사는 **Japan Post 하나**다. 국내11/12자리 숫자와 국제2영문자+9숫자+
2영문자 형식을 받으며 하이픈/소문자는 정규화한다. 공식 안내상 추적되지 않는 U로
시작하는 일반 소형우편 표기와 다른 배송사/형식은 지원하지 않는다. 로그인·Google·
브라우저 연결이나 추가 승인 창은 필요하지 않다.

실제 도구 `parcel_track`는 일본우편의 고정 HTTPS 조회 주소만 읽는다. 현재 확인한
영문 **최근 상태 요약 표**의 운송장 일치 행에서 날짜·상태·취급 우체국·지역과 원문
표시값을 구조화하고, 실제 조회 URL과 시각을 함께 남긴다. 전체 배송 이력을 읽었다고
주장하지 않는다. 날짜에 연도/UTC offset이 명확하지 않으면 임의의 연도·해외 시간대나
UTC 시각을 만들지 않고 원문 그대로 보여 준다. 배송 예정 시각을 추정하지 않는다.

공식 표가 찾지 못했다고 답하면 `not_found`, 확인한 표 구조가 달라지면
`provider_changed`로 구별한다. HTTP200만으로 배송 완료를 말하지 않는다. 네트워크나
응답 한도 오류도 실제 조회 실패이며 성공 결과로 꾸미지 않는다. 원문은 지시가 아닌
근거다. 일반 HTTP 읽기와 같은 공개 주소/DNS 고정·응답/시간 한도·취소/사용자/모델
권한 검사를 사용하고 cookie/credential/proxy·자동 retry·다른 host redirect는 쓰지 않는다.

현재 메시지에 새 번호가 명시되어 있으면 과거 번호보다 우선한다. 모델이 번호 인자를
비워도 일본우편의 유일한 현재 번호는 Store가 직접 사용하며, 서로 모순되거나 여러 번호로
모호하면 이전 번호로 조용히 되돌아가지 않고 번호를 확인한다.

그 뒤 같은 대화에서 `그 택배 지금 어디야?`라고 하면 마지막 실제 typed 조회 영수증의
유일한 번호를 다시 사용해 새로 조회할 수 있다. Store가 현재 요청 ID와 원래 번호가
나온 요청/run ID를 기록하며 모델이 ID를 만들어 주지 않는다. 최근32turn 안의 같은
계정/대화 영수증만 사용한다. 마지막 조회에 여러 번호가 있거나 영수증이 없으면 번호를
다시 묻고 다른 대화의 번호를 가져오지 않는다. 예전 상태를 현재 상태라고 재사용하지 않는다.

새 내장 `parcel-tracking` 절차의 정확한 pin이 없는 과거 실행에는 이 도구가 소급 추가되지
않는다. 읽기 설정을 끄면 이전 profile도 새 IO를 할 수 없다. 기본 web 설정이 없는
라이브러리 실행은 계속 unavailable이다. 반복 업무·배송 알림 구독·타 배송사 연결·주문/
취소·메일 발송·개인 인증 사이트는 이 도구의 범위가 아니다.

## 확인한 공개 근거와 검증 한계

2026-11-14에 다음 공식 공개 자료만 읽었다. 실제 사용자 운송장이나 credential은 쓰지 않았다.

- [공식 영문 입력 양식](https://trackings.post.japanpost.jp/services/srv/search/input?locale=en): GET form action, requestNo1/locale, 국내·국제 입력 예시.
- [현재 합성 all-zero 조회](https://trackings.post.japanpost.jp/services/srv/search?requestNo1=00000000000&locale=en&search.x=1&search.y=1): 실제 `Item number / Class of goods / Latest date / Latest status / Latest handling office / Prefecture / ZIP code` 표와 `Your item was not found` 응답.
- [공식 국제우편 주의사항](https://www.post.japanpost.jp/service/send/oversea/information/ems_search_en.html): 국제 번호 구성, U 표기 제외, 해외 자료 지연/누락과 시간대 차이 안내.

실제 성공 배송물의 최신 positive HTML이나 전체 history markup은 조회하지 않았다.
공개 양식에서 직접 확인한 summary header/cell 조건에 맞는 응답만 파싱하며 다른 모양이면
실패를 드러낸다. 합성 성공 표→실제 loopback HTTP/parser→Runtime/tool/SQLite→후속 조회와
원문 영수증 검증은 연결/경계 시험이다. 실제 Japan Post positive 조회 수락, 넓은 한국어 모델
이해 정확도·실서비스 안정성·타 배송사 지원 완료로 확대하지 않는다.
