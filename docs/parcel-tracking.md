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

전체 이력은 `일본우편 0000-0000-000 전체 배송 이력을 조회해줘`처럼 요청한다.
별도 도구 `parcel_history`는 고정 HTTPS `/services/srv/search/direct`에 정규화한
`reqCodeNo1`과 `locale=ja`만 붙여 **한 번** 읽는다. 사전 요약 읽기나 숨은 fallback은
없다. 실제 상세 표의 번호가 요청 번호와 같고, 단일 이력 표의 제목·열 순서와
rowspan/colspan, td/th 행 구성이 확인한 구조와 정확히 맞을 때만
`official_visible_history`를 반환한다. 날짜·상태·상세·취급국·지역·우편번호를 제공한 순서대로 보존하며 정렬하지
않는다. 최대64개 이벤트, 필드당512byte와 결과12KiB 이내다. 넘으면 일부 행을 성공으로
잘라 보여 주지 않고 변경된 구조/크기 실패를 드러낸다.

이 coverage는 **그 응답 페이지의 확인한 표시 이력**이다. 해외 자료 누락·지연과 시차로
전 세계 실제 취급 기록이 모두 들어 있다는 보장은 없다. 요약 표만 돌아오거나 번호가
다르거나 중복/깨진 이력 표면 `provider_changed`와 `history_unavailable`, 이벤트0으로
답한다. 최신 요약을 전체 이력 성공으로 대체하지 않는다. 확인한 정확한 일본어 부재 행은
`not_found`로 구별한다. 연도/offset 없는 시각에 연도·UTC·해외 시간대를 붙이지 않으며
예상 도착 시각을 만들지 않는다. `parcel_track`와 기존 개인 학습 v1은 계속 최근 요약만
사용한다. 이력 결과를 최신 요약 학습의 성공 증거로 승격하지 않는다.

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

내장 `parcel-tracking`1.1.0의 정확한 revision/digest pin이 없는 과거 실행에는
`parcel_history`의 도구 어휘나 실행 권한이 소급 추가되지 않는다. 읽기 설정을 끄면
이전 profile도 새 IO를 할 수 없다. 현재 exact pin 정책은 저장된1.0.0 실행에 최신 요약과
이력의 이중 legacy 권한을 유지하지 않는다.1.1.0 배포 후 새 조회는 새 대화/새 현재 pin
실행에서 시작한다.
요약의 응답 의미와 개인 학습 `japan-post-latest-v1` 구조는 바뀌지 않는다.
기본 web 설정이 없는 라이브러리 실행은 계속 unavailable이다. 반복 업무·배송 알림 구독·타 배송사 연결·주문/
취소·메일 발송·개인 인증 사이트는 이 도구의 범위가 아니다.

## 확인한 공개 근거와 검증 한계

2026-11-14에 다음 공식 공개 자료만 읽었다. 실제 사용자 운송장이나 credential은 쓰지 않았다.

- [공식 영문 입력 양식](https://trackings.post.japanpost.jp/services/srv/search/input?locale=en): GET form action, requestNo1/locale, 국내·국제 입력 예시.
- [현재 합성 all-zero 조회](https://trackings.post.japanpost.jp/services/srv/search?requestNo1=00000000000&locale=en&search.x=1&search.y=1): 실제 `Item number / Class of goods / Latest date / Latest status / Latest handling office / Prefecture / ZIP code` 표와 `Your item was not found` 응답.
- [공식 국제우편 주의사항](https://www.post.japanpost.jp/service/send/oversea/information/ems_search_en.html): 국제 번호 구성, U 표기 제외, 해외 자료 지연/누락과 시간대 차이 안내.

2026-11-15에 이력 구조를 다음 공개 자료와 대조했다. 실제 번호나 private shipment를
사용하지 않았고 제3자 운송장/원문을 저장소에 복사하지 않았다.

- [공식 직접조회의 합성 all-zero 일본어 부재 응답](https://trackings.post.japanpost.jp/services/srv/search/direct?reqCodeNo1=00000000000&locale=ja): 실제 direct GET와 정확한 일본어 요약 헤더/부재 행을 확인했다. 성공 이력 조회는 아니다.
- [공식 국제우편 주의사항과 설명용 이력 표](https://www.post.japanpost.jp/int/ems/delivery/ems_search_en.html): 날짜·상태·상세·취급국·지역/우편번호 배치와 해외 지연·누락·시차 주의사항을 확인했다. 이 기사 표를 현재 성공 응답으로 취급하지 않는다.
- [공식 일본어 주의사항의 설명용 이력 표](https://www.post.japanpost.jp/int/ems/delivery/ems_search.html):2026-11-15에 두 정확한 헤더 순서 `発生日 / 状態 / 詳細 / 取扱局名 / 県名等` 또는 `状態発生日 / 状態 / 詳細 / 郵便局名 / 県名等`, 둘째 행 `郵便番号`와 열0/1/2/4의rowspan2를 확인했다. 파서는 이 두 표기만 지원하며 미확인 header alias는 만들지 않는다. 현재 실제 상세 화면이 다른 표제를 사용하면 `history_unavailable`로 남는다. 설명용 표와 실제 상세 화면의 현재 동등성은 live 수락으로 확인하지 않았다.
- [haruhiko-zht/japanpost-tracking, revision91c0f506f463a40ba3525ad510e464e5d51c7dcd](https://github.com/haruhiko-zht/japanpost-tracking/blob/91c0f506f463a40ba3525ad510e464e5d51c7dcd/src/Tracking/TrackingScraping.php): `summary=履歴情報`, 두 헤더 행 이후5cell/1cell 행 쌍과 필드 위치를 확인했다.
- [kaosuna/japanpost-tracking-php, revision8c573697e398ca1101f7da1839ab9585f01555e7](https://github.com/kaosuna/japanpost-tracking-php/blob/8c573697e398ca1101f7da1839ab9585f01555e7/japanpost-tracking.php): direct GET, `srv_searchActionForm` 안의 `summary=配達状況詳細` 번호/상품 표와 같은 이력 행 구조를 독립적으로 대조했다.
- [hirorogo/japanpost-tracker, revision7bc200a7e4af77a288bd6eed382aba25623ebfd8](https://github.com/hirorogo/japanpost-tracker/blob/7bc200a7e4af77a288bd6eed382aba25623ebfd8/japanpost_tracker/scraper.py): 같은 direct endpoint와 상세/이력 summary 속성 및 짝지은 행 구조의 추가 공개 근거다.

파서는 위 구조를 바탕으로 독립 작성했고 성공 fixture의 모든 값은 새 합성 자료다.
이는 공개된 구조에 근거한 구현이지, 현재 공식 성공 배송물을 실제로 조회한 live 수락은
아니다. 현재 성공 markup이 바뀌면 실패를 드러낸다. 합성 표→실제 loopback HTTP/parser→
Runtime/tool/SQLite→후속 조회와 원문 영수증 검증은 연결/경계 시험이다. 실제 Japan Post
positive 조회 수락, 넓은 한국어 모델 이해 정확도·실서비스 안정성·타 배송사 지원 완료로
확대하지 않는다. [이력 구현 검토](reviews/2026-11-15-parcel-history.md)를 함께 본다.
