# 공개 자산 제작 기록

## 픽셀 시종 아이콘

`web/public/moosuri-attendant.png`은 2026-11-13 내장 imagegen 도구로 새로 생성한
원본 PNG(1254×1254)다. 기존 서비스의 그림·상표나 개인 이미지를 입력으로 사용하지
않았다. 색상은 프로젝트의 녹색·크림 계열을 따랐다. PWA와 Apple touch icon에 사용하며
maskable 인증을 주장하지 않는다. 빌드 때 원본 PNG bytes를 내장한 512×512 SVG icon
컨테이너를 생성한다. 원본 pixels는 변경하지 않으며 PWA manifest는 이 scalable icon을
가리킨다. Chromium이 큰 raster icon의 선언 크기를 설치 후보에서 제외하기 때문이다.

생성 prompt:

> Create an original app icon for moosuri, a self-hosted personal assistant. Use case: logo-brand. Cute pixel-art Korean palace attendant (musuri), a small friendly chibi female attendant in simple muted jade-green and cream hanbok, dark tidy hair bun with a small ribbon, warm peach face, two tiny dark eyes and gentle smile, holding a small folded cloth. No text, letters, logos, watermarks or existing characters. Square flat warm cream background matching #f6f4ed, restrained dark forest green #264b40 outlines with clean stepped pixels, not painterly, no gradients. Strong readable silhouette on a logical 32-by-32 pixel grid; crisp large pixels rendered cleanly. Center the entire character inside the central 70% of the canvas so circular/maskable app cropping preserves it. One character only, simple polished icon, no device mockup, no decorative UI.
