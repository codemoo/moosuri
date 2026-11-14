import { bindShareSession, fenceIncomingShares, readShareRow, shareEpoch, stageShare, validSharePayload, type SharePayload, type ShareSession } from "./share-state";
export async function boundedBody(body: ReadableStream<Uint8Array> | null, limit: number): Promise<Uint8Array<ArrayBuffer>> {
  if (!body) throw new Error("Missing body");
  const reader = body.getReader();
  const chunks: Uint8Array[] = [];
  let size = 0;
  let timedOut = false;
  const timer = setTimeout(() => { timedOut = true; void reader.cancel().catch(() => {}); }, 8000);
  try {
    while (true) {
      const { done, value } = await reader.read();
      if (timedOut) throw new Error("Body timed out");
      if (done) break;
      size += value.byteLength;
      if (size > limit) throw new Error("Body too large");
      chunks.push(value);
    }
    const bytes = new Uint8Array(size);
    let offset = 0;
    for (const chunk of chunks) { bytes.set(chunk, offset); offset += chunk.byteLength; }
    return bytes;
  } finally { clearTimeout(timer); void reader.cancel().catch(() => {}); reader.releaseLock(); }
}
export async function parseShare(request: Request): Promise<SharePayload> {
  const contentType = request.headers.get("Content-Type") ?? "";
  if (!/^multipart\/form-data;\s*boundary=/i.test(contentType) || contentType.length > 256) throw new Error("Invalid multipart");
  const data = await new Response(await boundedBody(request.body, 32768), { headers: { "Content-Type": contentType } }).formData();
  const payload: SharePayload = { title: "", text: "", url: "" };
  const seen = new Set<string>();
  data.forEach((value, key) => {
    if (!["title", "text", "url"].includes(key) || seen.has(key) || typeof value !== "string") throw new Error("Invalid share fields");
    seen.add(key); payload[key as keyof SharePayload] = value;
  });
  if (!validSharePayload(payload)) throw new Error("Invalid share text");
  return payload;
}
export async function freshShareSession(): Promise<ShareSession | null> {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), 8000);
  try {
    const response = await fetch("/api/v1/auth/session", { credentials: "same-origin", cache: "no-store", redirect: "error", signal: controller.signal });
    if (response.status === 401) return null;
    if (!response.ok) throw new Error("Session unavailable");
    const value = JSON.parse(new TextDecoder().decode(await boundedBody(response.body, 16384)));
    if (value.password_change_required || value.user?.must_change_password) {
      await fenceIncomingShares(true);
      throw new Error("Restricted session");
    }
    if (typeof value.user?.id !== "string" ||
      typeof value.chat_draft_scope !== "string" || !/^[0-9a-f]{64}$/.test(value.chat_draft_scope)) throw new Error("Unsupported session");
    return { account: value.user.id, scope: value.chat_draft_scope };
  } finally { clearTimeout(timer); }
}
/** Compare with durable generation captured BEFORE network work. */
export async function refreshShareSession() {
  const token = shareEpoch();
  const before = await readShareRow();
  const session = await freshShareSession();
  const row = await bindShareSession(before, session, token);
  return { row, session, token };
}
export async function receiveShare(request: Request): Promise<Response> {
  let fragment = "share-error=again";
  try {
    // Take the generation before parsing/authentication, so logout fences late intake.
    const token = shareEpoch(), before = await readShareRow();
    const payload = await parseShare(request);
    const session = await freshShareSession();
    const row = await bindShareSession(before, session, token);
    if (!row.account) fragment = "share-error=login";
    else fragment = `share=${await stageShare(row, payload, token)}`;
  } catch { /* Never put payload or provider/server errors in a redirect. */ }
  return new Response(null, { status: 303, headers: { Location: `/app#${fragment}`, "Cache-Control": "no-store" } });
}

/** Owner-authenticated read-only server proof. Original namespace is POST body,
 * never a URL parameter and never an authentication capability. */
export async function authorizeShareOrigin(session: ShareSession, origin: string): Promise<void> {
  if (!/^[0-9a-f]{64}$/.test(origin) || !/^[0-9a-f]{64}$/.test(session.scope)) throw new Error("Share origin unavailable");
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), 8000);
  try {
    const response = await fetch("/api/v1/shares/origin", {
      method: "POST", credentials: "same-origin", cache: "no-store", redirect: "error", signal: controller.signal,
      headers: { "Content-Type": "application/json", "X-Moosuri-Account": session.account },
      body: JSON.stringify({ origin_scope: origin, expected_current_scope: session.scope }),
    });
    if (response.status !== 204) throw new Error("Share origin unavailable");
  } finally { clearTimeout(timer); }
}
