# 복합 조사 보고서 구현 검증

2026-11-15, `2b115e8` 이후 A 단위. 합성 자료·임시 DB·격리 Linux 컨테이너로 검증했으며 실제 메일 발송이나 운영 DB 변경은 없다.

현재 요청의 조사/비교와 문서 저장 권한을 분리해 확인하고, 기존 미션 실행에서 읽은 근거와 계산 영수증을 한 비공개 문서의 실제 ID/revision에 연결한다. 원래 요청에 없던 문서 쓰기는 조건 수정으로 추가할 수 없다. 저장 결과는 내용의 의미 품질을 인증하지 않으며 검토 필요/partial로 남는다.

독립 검토에서 과거 읽기 미션의 쓰기 권한 확대, 편집한 보고서/자손의 원래 업무 철회 누락, 조건란의 계산 요구 누락, 기존 학습 절차의 실제 child receipt 연결 실패를 발견해 수정했다. 후속 검토의 일반 문구(product documentation/Add citations) 오인식과 숫자 덧셈 누락도 수정했다. 계산/출처 helper는 기존 검토 구현을 보존하며 최종 소스를 빌드·실행했다. 단순 산술 영수증은 산술만 증명하고 출처 숫자 사용은 evidence_calculate의 실제 읽기 연결로만 주장한다.

| 실제 검증 | 결과 |
| --- | --- |
| Linux Store `compound_report_` | 8 통과: 저장/ACK 유실/재시작, 소유권/이전 권한, 원문·편집/파생 문서 철회, 계산 요구/비교 문구, 학습 child receipt, schema58 이전 |
| 실제 Runtime의 합성 모델 transport | 1 통과: 현재 대화→위임 분류→계획→자료 2개 읽기·정확 계산→실제 문서→재개/철회 |
| 현재 sealed restore와 지원 predecessor restore | 각각 1 통과, 기존 권한/삭제 journal/격리 계약 유지 |
| React 맡긴 일 | 17 통과, 실제 저장 결과/검토 표시/문서 링크 포함 |
| TypeScript·프로덕션 웹 빌드·Rust Clippy | 통과 |

Linux 검증은 Rust1.89, 고정 image `sha256:7ba77c3edd65abe571942727dcdf8fc7eb53a57a83832d8313d7a48be0954991`, network-none/rootless 컨테이너에서 수행했다. 최종 source archive SHA256은 `2c7ea0484ae1d9a6a87256f80347726e3a0f1aef0c86d5f6d2d1c16f9e682f2b`; 검증 중 source hash도 일치했다. 오래된 합성 DB downgrade fixture는 새 schema59 테이블을 제거하도록 갱신했다.

A는 지원 자료의 읽기·계산과 요청한 private 문서 저장까지다. 계정 선택 후 재개, native Gmail 모델 읽기, 미션의 메일/Calendar 실행과 새로운 복합 학습은 별도 단위다. 이 합성 회귀는 독립 실모델 holdout·실제 provider/device 수락·안정판 출시·운영 이전을 대신하지 않는다. GitHub Actions/자동 CI는 계속 꺼져 있다.
