# Design

Vite embeds an exact public-asset allowlist and a hash of its bytes into a classic
TypeScript service worker. Installation fetches anonymously and rejects redirects.
Only the built index, immutable build chunks and declared public icons/manifest
enter this cache. Private API and media paths, mutations and foreign origins are
never intercepted. Navigations to known app shells use the network, with cached
public index on network failure. Credential-bearing navigation responses are not
written to the cache.

The worker does not force activation or reload an existing tab. Normal activation
removes only older moosuri public-shell caches. A production secure context registers
the worker; browser installation remains a user gesture. The UI announces offline
status without replacing mounted draft state. An initially offline boot retries
authentication on reconnect. Safari home-screen installation uses its native share
menu; browser support and actual device acceptance remain separate evidence.
