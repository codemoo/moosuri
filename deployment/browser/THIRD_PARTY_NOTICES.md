# Vendored browser sandbox profile

`seccomp-upstream.json` is an unchanged copy of the Playwright v1.56.1 Docker
seccomp profile, maintained by Microsoft Corporation and the Playwright
contributors. `seccomp.json` is a moosuri-derived profile with one modification:
unconditionally permit the `chroot` syscall so Chromium's zygote can chroot after
entering its user namespace without a host CAP_SYS_CHROOT capability. The original
capability-conditional rule remains. No container capability is added; all five
capability sets must be zero before browser launch, NoNewPrivs remains set and
non-namespaced `chroot("/")` must fail EPERM in the containment smoke.

- Upstream: <https://github.com/microsoft/playwright/blob/v1.56.1/utils/docker/seccomp_profile.json>
- License: Apache License, Version 2.0. The complete upstream license is included
  as [LICENSE-Playwright-Apache-2.0.txt](LICENSE-Playwright-Apache-2.0.txt).
- Unchanged upstream SHA-256:
  `cc3e61cabda6bbc1e53e54d27ba4d55a9d3be829b6dd1a596f4a7b31b1cc7849`.
- Derived profile SHA-256:
  `18c6523f4de6aebb9d220d98fea8a03edb381d97237f954ed9d1f47d233f661c`.

This attribution and the included license apply to both profiles; they do not
imply Microsoft or Playwright endorses moosuri or its containment claims.
