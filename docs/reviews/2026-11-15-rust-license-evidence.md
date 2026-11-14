# Offline Rust notice evidence acceptance

The P10 unit adds `xtask rust-notices --vendor DIRECTORY [--require-complete]`.
It reads explicit public vendored dependencies only, preserves package attribution
and license declarations, binds bytes to the committed lock/local checksum
metadata and exposes missing evidence. It does not choose licenses, authenticate
registry downloads or certify binary/distribution legal compliance.

The selected Rust1.89 Linux x86_64 container ran with network disabled and public
source/vendor mounts read-only. Three focused tests passed: distinct versions and
exact Unicode attribution/determinism; missing metadata plus changed package/hash
and traversal rejection; observed package/notice symlink rejection. Affected
all-target Clippy passed. Actual generation against the existing434-package vendor
tree succeeded, preserving784 candidate notices and4,085,403 raw text bytes.
Twenty-one packages had no selected notice file; no empty/missing notice was
reported as complete or replaced by an inferred license template. The actual strict
CLI emitted that complete report and exited1 for those21 missing files.

The missing package versions are age0.11.5, age-core0.11.0, alloc-stdlib0.2.4,
bech320.9.1, cookie-factory0.3.3, ece-native0.5.0, fluent-langneg0.13.1,
intl_pluralrules7.0.2, io_tee0.1.1, jni0.22.4, jni-macros0.22.4,
jni-sys-macros0.4.1, r-efi5.3.0/6.0.0, rustls-platform-verifier-android0.2.0,
selectors0.41.0, ts-rs11.1.0, ts-rs-macros11.1.0, web-push-native0.5.0 and
winapi-i686-pc-windows-gnu/winapi-x86_64-pc-windows-gnu0.4.0.
This complete source-lock list includes non-Linux and development dependencies;
it is not evidence that every listed package is present in a shipped binary.

No dependency, runtime, Store schema, existing signed format, credentials, private
source, production service or CI setting changed. Full notices, npm/asset coverage,
license/source obligations and official artifact acceptance remain release gates.
