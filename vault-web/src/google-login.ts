import type { Command, Summary } from "./types";

export type GoogleLogin = Summary & { email: string };
export const validGoogleEmail = (email: unknown): email is string => typeof email === "string" &&
  email.length <= 254 && /^[^@\s]+@[^@\s]+$/u.test(email) && !/[\r\n\0]/u.test(email);

export function googleAccounts(read: <T>(command: Command) => T): GoogleLogin[] {
  return read<Summary[]>({op:"list",query:""}).flatMap(item => {
    if (item.kind !== "login") return [];
    const email = read<string>({op:"reveal",item:item.id,field:"username"});
    const address = read<string>({op:"reveal",item:item.id,field:"url"});
    return validGoogleEmail(email) && isGoogleLogin(address,email,email) ? [{...item,email}] : [];
  });
}

const googleOrigin = "https://accounts.google.com";

// The stored address is evidence for selecting an item. It is never navigated to.
export function isGoogleLogin(address: string, username: string, email: string) {
  if (!email || username.toLowerCase() !== email.toLowerCase() || /[\\\s]/u.test(address)) return false;
  try {
    const url = new URL(address);
    return url.origin === googleOrigin && !url.username && !url.password;
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
