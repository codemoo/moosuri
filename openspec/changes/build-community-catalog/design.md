## Provenance and identity

A validated Selection contains canonical public GitHub repository, one full lowercase commit and 1–16 unique directories. The only public generation path takes this selection and obtains the existing private DownloadedGitHubSkill carrier from the fixed public API adapter; local bytes and user-supplied claimed provenance have no input API. The adapter resolves/verifies matching commit and tree/blob identity and bounds file layout/modes/bytes. Shared external conversion and installer validation create the same v2 envelope and digest as preview. Curated v1 bytes, source constant and pin manifest remain unchanged.

## Artifacts

Generate compact existing v2 JSON envelopes as package-<content_digest>.json and catalog.json with sorted directory entries, compatible public metadata, canonical source commit, v2 identity and separate SHA-256 of artifact bytes. No timestamps/local paths/self-referential index Git commit occur. No new consumer/activation endpoint is implied. Ordinary users continue installing selected public GitHub commits through the existing UI.

## Bounds and publication

Maximum16 packages, each existing12-request/2MiB/30s adapter acquisition and32KiB/file/64KiB package bounds; total120s deadline and8MiB artifact output limit. Collect and validate all packages before producing a complete artifact set. CLI validates a new ordinary output destination before acquisition; it opens no Store/data-dir and prints no source text. Reuse init's descriptor traversal, installation-marker rejection, private sibling staging, sync, no-replace rename and tracked-inode cleanup. Retain pinned descriptors for every existing ancestor. Each must be root/current-effective-UID owned, with no group/world write unless sticky. Trusted root/current-owned sticky temp is supported only with continuing child ownership checks. Recheck the entire chain before publication and conservative cleanup. Root/current-UID concurrent operator mutations remain trusted. Reject macOS ACL grants (deny-only supported) and detected Linux extended/default/NFSv4 directory ACLs; ACL inspection failure rejects with a static diagnostic, without claiming arbitrary filesystem policy authentication. This tighter boundary also applies to existing init/check. Prior output is never replaced; parent-sync error may follow complete publication. Failures have bounded static diagnostics.

Private data/installation/learning reads, hooks/scripts, release network writes, automatic consumers and curated pin rewriting remain outside this operation.
