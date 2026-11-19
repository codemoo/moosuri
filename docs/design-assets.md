# 무수리의 화면과 아이콘

## 조선 왕실 색상

| 화면 역할 | 색상 | 참고 유물 |
| --- | --- | --- |
| 화면 바탕과 카드 | `#151515`, `#212120` | 검은 옻칠로 마감한 왕실 문방구 [흑칠나전연상](https://www.gogung.go.kr/gogung/pgm/psgudMng/view.do?psgudSn=438958&menuNo=800065)과 영조·정조의 글씨를 인쇄한 [영은경희록](https://www.gogung.go.kr/gogung/pgm/psgudMng/view.do?psgudSn=366176&menuNo=800065) |
| 선택 상태와 주요 버튼 | `#C4D0DF`, `#29313A` | 청색을 비롯한 단청 안료를 쓴 어좌 뒤 [일월오봉도](https://www.gogung.go.kr/gogung/pgm/psgudMng/view.do?psgudSn=363400&menuNo=800070) |
| 본문과 제목 | `#F2EFE9` | 조선 백자의 따뜻한 유백색 [백자 달항아리](https://www.museum.go.kr/MUSEUM/contents/M0502000000.do?schM=view&relicId=941) |
| 삭제와 오류 | `#EDA394`, `#3A2522` | 일월오봉도의 홍색 안료 |

표의 HEX는 국립고궁박물관과 국립중앙박물관의 유물 해설을 바탕으로 다크 UI에서 읽기 쉽게 조정한 화면용 색상이다. 무채색 바탕에 푸른 회색을 조작 강조색으로 쓰고, 청록은 브랜드 아이콘에 담았다. 색상 토큰은 [workspace.css](../web/src/workspace.css)에서 관리한다.

## 글꼴과 워드마크

로그인과 작업 공간의 브랜드 표기는 아이콘과 함께 **Moosuri**를 사용한다. 일반 텍스트, 한글 입력창, Astryx 컨트롤은 Pretendard Variable 1.3.9의 가변 굵기를 공유한다. 폰트는 원본 WOFF2를 웹 서버에서 제공하며 PWA 앱 셸에 함께 캐시한다. ASCII와 한글을 모두 포함한다. [Pretendard 1.3.9](https://github.com/orioncactus/pretendard/releases/tag/v1.3.9) · [SIL Open Font License](../web/src/fonts/OFL-Pretendard.txt)

## 화면 배치

Muse의 데스크톱 화면(1440×900)에서 72px 탐색 레일, 중앙에 가까운 탐색 항목, 아래쪽 768×58px 한 줄 입력창을 확인했다. 모바일(390×844)은 좌우 12px 여백에서 한 줄 입력을 유지했다. 무수리는 Astryx의 `AppShell`, `TopNav`, `SideNav`, `SideNavItem`, `Dialog`, `ChatComposer`로 이 배치를 구현했다. 모바일은 왼쪽 위의 무수리 아이콘에서 탐색·이전 대화·설정을 열고, 아래쪽에는 입력창을 둔다. 입력창의 본문 슬롯에서 한국어 조합 입력과 계정별 초안 저장을 처리한다. [Muse](https://muse.ai/) · [Astryx v0.7.16](https://github.com/meta/astryx/tree/v0.7.16)

회원별 Kiri 배포에서 공개 주소는 `web/personal.html`을 제공한다. 이 진입점과 기존 웹 진입점은 같은 Astryx Neutral 테마, Pretendard 글꼴, 무수리 색상, 뷰포트 처리를 사용한다. Kiri 화면의 내비게이션과 입력창에는 개인 서비스에서 실행할 수 있는 동작만 표시한다.

## 아이콘

2026-11-18 ImageGen으로 접힌 옷깃을 추상화했다. Muse 파비콘의 간결한 윤곽과 Pickle의 작고 둥근 브랜드 형태를 참고했다. 그림에는 넓은 청록 띠 두 줄과 안쪽으로 접히는 선 하나만 들어간다. [Muse 파비콘](https://muse.ai/favicon.ico) · [Pickle 앱 소개](https://pickle.com/og/main_thumbnail_agent.jpg)

제작 지시 요약. 정사각형 안에서 접힌 한복 깃을 넓은 띠 두 줄과 작은 접힘으로 표현한다. 옅은 청록 `#ADDCC2`과 먹에 가까운 `#101719`를 사용한다. 파비콘 크기에서도 윤곽이 남도록 중앙 여백을 넉넉히 두고 글자와 장식을 생략한다.

- [192px 아이콘](../web/public/moosuri-icon-192.png)은 웹, Apple touch icon, 데스크톱 메뉴에서 쓴다.
- [512px 아이콘](../web/public/moosuri-icon-512.png)은 PWA 설치에 쓴다.
- [픽셀 시종 원본](moosuri-attendant-original.png)은 초기 디자인 기록으로 보관한다.
