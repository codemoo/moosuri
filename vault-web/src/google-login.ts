import type { Command, Summary } from "./types";

const googleAccounts = "https://accounts.google.com";

// The stored address is evidence for selecting an item. It is never navigated to.
export function isGoogleLogin(address: string, username: string, email: string) {
  if (!email || username.toLowerCase() !== email.toLowerCase() || /[\\\s]/u.test(address)) return false;
  try {
    const url = new URL(address);
    return url.origin === googleAccounts && !url.username && !url.password;
  } catch { return false; }
}

export function googleLogins(email: unknown, read: <T>(command: Command) => T) {
  if (typeof email !== "string" || email.length > 254 || !/^[^@\s]+@[^@\s]+$/u.test(email)) throw Error("vault_invalid_input");
  const candidates = read<Summary[]>({ op: "list", query: email });
  return candidates.filter(item => {
    if (item.kind !== "login") return false;
    const username = read<string>({ op: "reveal", item: item.id, field: "username" });
    if (username.toLowerCase() !== email.toLowerCase()) return false;
    const address = read<string>({ op: "reveal", item: item.id, field: "url" });
    return isGoogleLogin(address, username, email);
  });
}
