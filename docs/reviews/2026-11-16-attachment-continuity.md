# Attachment preparation and inventory continuity

Bounded synthetic frontend pass from `6892673`. Source ownership is Media.tsx plus its Media.test.tsx and PrivateFiles.test.tsx observations. App.tsx, ChatPanel.tsx, API transport, voice implementation, Rust, model/effort and CI are unchanged. No Cargo, production, live model/provider or real-credential calls occurred. Integration, independent review, browser smoke and publication belong to the parent.

## Reproduced defects and fixes

1. Two selected images shared a preparation promise queue that also awaited the first upload response. With the first POST held, the second image never even prepared; the reproduction observed only one XHR and failed its expectation of two independent uploads. The queue now serializes decoding/resizing only, then starts the existing per-item upload/error/controller path independently. The focused observation holds bitmap preparation first to prove decoder concurrency remains one, then holds the first network receipt while the second becomes ready. Sending the ready selection preserves the pending first image and text; its later response remains usable.
2. Library refreshes could publish out of order. An older retry_required inventory arriving after a ready inventory removed the Attach control and demanded the original file again. The reproduction failed before the fix. Refreshes now have an immutable current read controller and account-generation fence; a newer refresh aborts the previous read, and only its owner can publish inventory/error or clear ownership. Scope cleanup cancels the current read. Upload recovery controllers, original UUID/bytes and deletion filtering remain independent.

## Evidence

- Before source changes, both new focused observations failed on the base implementation: held first upload stranded the second image; older inventory replaced ready with retry_required.
- `npm test -- --run src/Media.test.tsx src/PrivateFiles.test.tsx src/VoiceComposer.test.tsx`: 31 passed across three affected suites.
- After strengthening the decoder observation to hold preparation before network, its focused rerun passed with 9 unrelated tests skipped. No new implementation changes followed the broad suite/build.
- `npm run build` passed app/worker TypeScript and Vite production output.
- Pinned OpenSpec 2.0.0 strict named validation of attachment-upload-continuity passed with telemetry disabled.
- `git diff --check` passed.

Existing affected-suite observations cover pending image/file independence from text, original-UUID/Blob retry, cancellation and late ACK, deselect/reselect ownership, switching draft selection, account-keyed unmount, deletion/recovery fencing, and immutable voice receipt recovery. These are regression observations, not newly claimed defects. No older deletion/run-retry suites or unrelated backend checks were rerun.

## Limits

This pass fixes two observed attachment defects. It adds no upload timeout or new persistence/cache. A full reload or navigating away from the mounted composer can still retire local unsent selection/recording state; completed server attachments remain in the existing library. Raw uploads/voice are not promised crash recovery. Real browser/device preparation throughput, mobile microphone interruptions, actual provider accuracy and network latency remain outside these tests. Existing four-selection and server capacity bounds continue to apply.
