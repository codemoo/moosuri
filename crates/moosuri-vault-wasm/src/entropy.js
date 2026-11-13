// Trusted Worker import. Missing browser CSPRNG is fatal; there is no fallback.
export function fillRandom(output) {
  if (!globalThis.crypto || typeof globalThis.crypto.getRandomValues !== "function") {
    throw new Error("vault_entropy_failed");
  }
  for (let at = 0; at < output.length; at += 65536) {
    globalThis.crypto.getRandomValues(output.subarray(at, Math.min(at + 65536, output.length)));
  }
}
