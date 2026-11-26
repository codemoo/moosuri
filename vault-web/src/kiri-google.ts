import type { Command, Summary } from "./types";
import {validGoogleEmail} from "./google-login";

export type KiriRecipient = { account: string; email: string; spki: string };
export const validJob = (id: unknown): id is string => typeof id === "string" &&
  /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(id);
const encode = (data: ArrayBuffer | Uint8Array) => btoa(String.fromCharCode(...new Uint8Array(data as ArrayBuffer)));

export async function sealGoogleLogin(recipient: KiriRecipient, account: string, email: string, job: string,
  item: Summary & {email:string}, run: <T>(command: Command) => Promise<T>) {
  if (!validJob(job) || recipient.account !== account || recipient.email !== email || !validGoogleEmail(item.email)) throw Error("vault_unauthorized");
  const label = new TextEncoder().encode(["moosuri-google-v2", job, account, email, item.email].join("\n"));
  const publicKey = await crypto.subtle.importKey("spki", Uint8Array.from(atob(recipient.spki), c => c.charCodeAt(0)),
    { name: "RSA-OAEP", hash: "SHA-256" }, false, ["encrypt"]);
  if ((publicKey.algorithm as RsaKeyAlgorithm).modulusLength !== 3072) throw Error("vault_unavailable");
  const key = await crypto.subtle.generateKey({name:"AES-GCM", length:256}, true, ["encrypt"]);
  const raw = await crypto.subtle.exportKey("raw", key);
  const wrapped = await crypto.subtle.encrypt({name:"RSA-OAEP", label}, publicKey, raw);
  new Uint8Array(raw).fill(0);
  const iv = crypto.getRandomValues(new Uint8Array(12));
  const secret = new TextEncoder().encode(await run<string>({op:"google_fill", email:item.email, item:item.id, revision:item.revision}));
  try {
    if (!secret.length || secret.length > 4096) throw Error("vault_invalid_input");
    const body = await crypto.subtle.encrypt({name:"AES-GCM", iv, additionalData:label}, key, secret);
    return {key:encode(wrapped), iv:encode(iv), body:encode(body)};
  } finally { secret.fill(0); }
}
