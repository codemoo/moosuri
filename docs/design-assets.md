# 공개 자산 제작 기록

현재 아이콘은 2026-11-18 내장 ImageGen으로 제작했다. 기존 무수리 시종의 쪽진 머리,
녹색 댕기, 한복 깃을 유지하고, 화면에서 작게 쓸 수 있도록 얼굴과 어깨를 중심으로
재구성했다. 공개 [Muse 파비콘](https://muse.ai/favicon.ico)의 간결한 윤곽과
[Pickle 캐릭터](https://pickle.com/og/main_thumbnail_agent.jpg)의 표정이 시각 참고였다.
ImageGen에는 무수리 원본만 입력했다.

- [192px 아이콘](../web/public/moosuri-icon-192.png)은 파비콘, 채팅, Apple touch icon, 데스크톱 메뉴에서 사용한다.
- [512px 아이콘](../web/public/moosuri-icon-512.png)은 PWA 설치에서 사용한다.
- [기존 픽셀 시종 원본](moosuri-attendant-original.png)은 제작 참고용이다.

> Use case: identity-preserve. Generate a square production favicon and app avatar for Moosuri, a Korean personal assistant. Use the attached original Moosuri sprite as an identity reference. Reinterpret her as a compact friendly head-and-shoulders icon, keeping dark top bun, ivory hair tie, muted jade ribbon tucked to the right, warm peach face, two immediately legible eyes, small smile, jade Korean attendant hanbok and broad diagonal ivory collar. Borrow only high-level principles of modern AI companion branding: one decisive readable silhouette, a sweeping expressive hair line and instantly readable pair of eyes. Handheld-console pixel art with intentional chunky stepped edges and at most 10 flat colors. Center the icon on a perfectly flat solid dark pine charcoal #162725 tile that fills the square to all four edges. Use even breathing room: avatar around 68 percent of canvas height, 65 percent width, head and shoulders. Keep the figure visible on the dark background using a small jade edge highlight around dark hair. Palette jade, pine-charcoal, ivory and peach. No words, letters, decorative stars, gradient, noise, grain, glossy shading, outer border or white tile. The original human character stays distinct. Crisp, cute and unmistakable even at 28 pixels.

## 2026-11-13 원본 제작 프롬프트

> Create an original app icon for moosuri, a self-hosted personal assistant. Use case: logo-brand. Cute pixel-art Korean palace attendant (musuri), a small friendly chibi female attendant in simple muted jade-green and cream hanbok, dark tidy hair bun with a small ribbon, warm peach face, two tiny dark eyes and gentle smile, holding a small folded cloth. No text, letters, logos, watermarks or existing characters. Square flat warm cream background matching #f6f4ed, restrained dark forest green #264b40 outlines with clean stepped pixels, not painterly, no gradients. Strong readable silhouette on a logical 32-by-32 pixel grid; crisp large pixels rendered cleanly. Center the entire character inside the central 70% of the canvas so circular/maskable app cropping preserves it. One character only, simple polished icon, no device mockup, no decorative UI.
