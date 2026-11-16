## Implementation
- [x] Reproduce a held first upload stranding a second image before preparation.
- [x] Reproduce an older inventory replacing a newer ready source with retry_required.
- [x] Serialize bitmap work only; preserve independent upload ownership and ready-only sending.
- [x] Fence library refresh success/error by its latest immutable read owner and scope.

## Verification
- [x] Three affected frontend suites passed 31 tests; production app/worker TypeScript and Vite build passed.
- [x] Strict named OpenSpec validation, final diff check and dedicated evidence note.
