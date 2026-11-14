import "./incoming-share.css";
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { appendShare, claimShare, consumeShare, deliverShare, discardShare, onShareRetirement, readShareRow, shareEpoch, shareId, shareText, type ShareClaim } from "./share-state";
import { authorizeShareOrigin, refreshShareSession } from "./share-intake";
const failure = "공유를 가져올 수 없습니다. 로그인 상태를 확인한 뒤 다시 공유해 주세요.";
export function shareFragment(hash: string): string | null {
  const match = /^#share=([0-9a-f-]+)$/i.exec(hash);
  return match && shareId(match[1]) ? match[1] : null;
}
export function IncomingShareNotice() {
  const hash = window.location.hash;
  return hash.startsWith("#share") ? <p role="status">{shareFragment(hash)
    ? "공유가 대기 중입니다. 원래 계정으로 로그인한 뒤 대화에서 직접 가져와 주세요. 5분 후 만료됩니다."
    : hash === "#share-error=login" ? "로그인한 뒤 다시 공유해 주세요." : failure}</p> : null;
}
export function IncomingShare({ accountId, sessionScope, slot, draft, insert, moveSlot }: {
  accountId: string; sessionScope: string | null; slot: string; draft: () => string;
  insert: (slot: string, text: string) => void;
  moveSlot?: { current: ((from: string, to: string) => void) | null };
}) {
  const [id, setId] = useState(() => shareFragment(window.location.hash));
  const [claim, setClaim] = useState<ShareClaim | null>(null);
  const [text, setText] = useState("");
  const [message, setMessage] = useState("");
  const [busy, setBusy] = useState(false);
  const windowId = useRef(crypto.randomUUID());
  const originalSlot = useRef<string | null>(null);
  const alive = useRef(true);
  const operation = useRef(0);
  const current = useRef({ slot, accountId, sessionScope, draft, insert });
  current.current = { slot, accountId, sessionScope, draft, insert };
  useLayoutEffect(() => {
    if (!moveSlot) return;
    const move = (from: string, to: string) => { if (originalSlot.current === from) originalSlot.current = to; };
    moveSlot.current = move;
    return () => { if (moveSlot.current === move) moveSlot.current = null; };
  }, [moveSlot]);
  useLayoutEffect(() => {
    operation.current++; setClaim(null); setText(""); setBusy(false);
  }, [accountId, sessionScope]);
  useEffect(() => {
    alive.current = true;
    const changed = () => { operation.current++; setId(shareFragment(window.location.hash)); setClaim(null); setText(""); setBusy(false); setMessage(""); };
    const hide = () => { operation.current++; setClaim(null); setText(""); setMessage(failure); };
    window.addEventListener("hashchange", changed); window.addEventListener("popstate", changed);
    window.addEventListener("pagehide", hide);
    const stopRetirement = onShareRetirement(hide);
    return () => { stopRetirement(); alive.current = false; operation.current++; window.removeEventListener("hashchange", changed); window.removeEventListener("popstate", changed); window.removeEventListener("pagehide", hide); };
  }, []);
  useEffect(() => {
    if (!claim) return;
    const timer = setTimeout(() => {
      operation.current++; setClaim(null); setText(""); setBusy(false); setMessage(failure);
      void readShareRow().catch(() => {}); // expiry erases stored payload on access
    }, Math.max(0, claim.expires - Date.now()));
    return () => clearTimeout(timer);
  }, [claim]);
  async function importShare() {
    if (!id || busy) return;
    const op = ++operation.current;
    setBusy(true); setMessage("");
    try {
      const { row, session, token } = await refreshShareSession();
      if (!alive.current || op !== operation.current || !session || session.account !== accountId || session.scope !== sessionScope) throw new Error();
      const result = await claimShare(row, id, windowId.current, session, token);
      const delivered = await refreshShareSession();
      if (!delivered.session || delivered.session.account !== session.account || delivered.session.scope !== session.scope) throw new Error();
      try { await authorizeShareOrigin(delivered.session, result.originalScope); }
      catch (error) { await discardShare(result); throw error; }
      await deliverShare(result, "claimed", () => {
        if (!alive.current || op !== operation.current || token !== shareEpoch()) throw new Error();
        originalSlot.current = current.current.slot;
        setClaim(result); setText(shareText(result.payload));
      }, token);
    } catch { if (alive.current && op === operation.current) setMessage(failure); }
    finally { if (alive.current && op === operation.current) setBusy(false); }
  }
  async function insertShare() {
    if (!claim || busy || current.current.slot !== originalSlot.current) return;
    const op = ++operation.current;
    setBusy(true); setMessage("");
    try {
      // A fresh server check also precedes insertion; a cached UI session is insufficient.
      const { session, token } = await refreshShareSession();
      if (!session || session.account !== claim.session.account || session.scope !== claim.session.scope ||
        !alive.current || op !== operation.current || current.current.slot !== originalSlot.current) throw new Error();
      if (appendShare(current.current.draft(), text) === null) { setMessage("입력할 수 있는 길이를 넘었습니다. 공유 글을 조금 줄여 주세요."); return; }
      try { await authorizeShareOrigin(session, claim.originalScope); }
      catch (error) { await discardShare(claim); throw error; }
      await consumeShare(claim, token);
      // Consumption completion may be suspended while another context retires.
      const delivered = await refreshShareSession();
      if (!delivered.session || delivered.session.account !== claim.session.account || delivered.session.scope !== claim.session.scope) throw new Error();
      await authorizeShareOrigin(delivered.session, claim.originalScope);
      await deliverShare(claim, "consumed", () => {
        if (!alive.current || op !== operation.current || token !== shareEpoch() || current.current.slot !== originalSlot.current) throw new Error();
        // Re-read latest composer in the transaction: text typed meanwhile is retained.
        const next = appendShare(current.current.draft(), text);
        if (next === null) throw new Error();
        current.current.insert(originalSlot.current!, next);
        setClaim(null); setText(""); setId(null); setMessage("초안에 넣었습니다. 내용을 확인하고 보내기를 눌러 주세요.");
        window.history.replaceState(null, "", window.location.pathname + window.location.search);
      }, delivered.token);
    } catch { if (alive.current && op === operation.current) { setClaim(null); setText(""); setMessage(failure); } }
    finally { if (alive.current && op === operation.current) setBusy(false); }
  }
  if (!id && !message && !window.location.hash.startsWith("#share")) return null;
  if (!id && message) return <p className="incoming-share-status" role="status">{message}</p>;
  return <section aria-label="공유받은 글" className="incoming-share">
    <h3>공유받은 글</h3>
    {message && <p role="status">{message}</p>}
    {!claim ? id && <><p>이 계정으로 가져온 뒤 편집할 수 있습니다. 자동으로 보내지 않습니다.</p><button type="button" disabled={busy} onClick={() => void importShare()}>이 계정의 초안으로 가져오기</button></> : <>
      <label htmlFor="incoming-share-text">공유 글 편집</label>
      <textarea id="incoming-share-text" rows={4} disabled={busy} value={text} onChange={e => setText(e.target.value)} />
      {appendShare(draft(), text) === null && <p role="status">입력할 수 있는 길이를 넘었습니다. 공유 글을 조금 줄여 주세요.</p>}
      {slot !== originalSlot.current && <p role="status">처음 가져온 대화로 돌아온 뒤 초안에 넣어 주세요.</p>}
      <button type="button" disabled={busy || slot !== originalSlot.current || appendShare(draft(), text) === null} onClick={() => void insertShare()}>초안에 넣기</button>
      <p>새로고침하거나 화면을 떠난 뒤에는 다시 공유해야 할 수 있습니다.</p>
    </>}
    {!id && !message && <IncomingShareNotice/>}
  </section>;
}
