# Vendored browser sandbox profile

`seccomp-upstream.json` is an unchanged copy of the Playwright v1.56.1 Docker
seccomp profile, maintained by Microsoft Corporation and the Playwright
contributors. `seccomp.json` is a moosuri-derived profile. Its initial modification:
unconditionally permit the `chroot` syscall so Chromium's zygote can chroot after
entering its user namespace without a host CAP_SYS_CHROOT capability. The original
capability-conditional rule remains. No container capability is added; all five
capability sets must be zero before browser launch, NoNewPrivs remains set and
non-namespaced `chroot("/")` must fail EPERM in the containment smoke.

For Kiri's profile parser, an unconditional rule's `args: null` is represented
as `args: []`. An explicit `clone3` denial returns ENOSYS (38), allowing libc to
fall back to the filtered `clone` syscall. No syscall is newly allowed by these
compatibility changes. The test worker checks the actual clone3 denial.

- Upstream: <https://github.com/microsoft/playwright/blob/v1.56.1/utils/docker/seccomp_profile.json>
- License: Apache License, Version 2.0. The complete upstream license is included
  as [LICENSE-Playwright-Apache-2.0.txt](LICENSE-Playwright-Apache-2.0.txt).
- Unchanged upstream SHA-256:
  `cc3e61cabda6bbc1e53e54d27ba4d55a9d3be829b6dd1a596f4a7b31b1cc7849`.
- Derived profile SHA-256:
  `25a51bedbc152a36eb3ec4557231c18928f640ea61c9131499b3bc5491e03dc2`.
- Previously deployed Docker profile, still accepted by the launcher:
  `18c6523f4de6aebb9d220d98fea8a03edb381d97237f954ed9d1f47d233f661c`.

This attribution and the included license apply to both profiles; they do not
imply Microsoft or Playwright endorses moosuri or its containment claims.
