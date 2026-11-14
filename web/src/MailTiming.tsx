import { useEffect, useRef, useState } from "react";
import type { MailAction, MailSchedule } from "../shared/api-types";
import type { RequestScope } from "./api";
export function formatMailTime(s: MailSchedule) {
  return `${new Intl.DateTimeFormat("ko-KR", { timeZone: s.timezone, dateStyle: "full", timeStyle: "long" }).format(s.due_at * 1000)} · ${s.timezone} · UTC ${new Date(s.due_at * 1000).toISOString()}`;
}
export function MailTiming({ action, request, changed, editing, fail, disabled }: {
  action: MailAction;
  request: RequestScope["request"];
  changed: (a: MailAction) => void;
  editing: (v: boolean) => void;
  fail: (e: unknown) => void;
  disabled: boolean;
}) {
  const [open, setOpen] = useState(false);
  const [local, setLocal] = useState("");
  const [zone, setZone] = useState(Intl.DateTimeFormat().resolvedOptions().timeZone);
  const [preview, setPreview] = useState<MailSchedule | null>(null);
  const [busy, setBusy] = useState(false);
  const [generation, setGeneration] = useState(0);
  const live = useRef(true);
  useEffect(() => { live.current = true; return () => { live.current = false; }; }, []);
  function invalidate() { setPreview(null); setGeneration(v => v + 1); }
  async function save(schedule: MailSchedule | null) {
    setBusy(true);
    try {
      const a = await request<MailAction>(`/mail/actions/${encodeURIComponent(action.id)}/schedule`, "PUT", { expected: { revision: action.revision, digest: action.digest }, schedule });
      if (!live.current) return;
      changed(a); setOpen(false); editing(false);
    } catch (e) { if (live.current) fail(e); } finally { if (live.current) setBusy(false); }
  }
  return <section aria-label="메일 예약 시각">
    <p>{action.schedule ? `예약 발송: ${formatMailTime(action.schedule)}` : "지금 발송 · 최종 허락 후 실행"}</p>
    <p>예약은 고정한 한 통만 보냅니다. 로그아웃 후에도 승인한 예약은 유지되며, 시각이나 내용을 바꾸면 다시 전체 확인이 필요합니다. 최대 24시간 지연 후에는 만료됩니다.</p>
    {!open ? <button disabled={disabled} onClick={() => { setOpen(true); editing(true); }}>예약 시각 설정·변경</button> : <div>
      <label>발송할 날짜와 현지 시각<input type="datetime-local" value={local} disabled={busy} onChange={e => { setLocal(e.target.value); invalidate(); }} /></label>
      <label>IANA 시간대<input value={zone} disabled={busy} onChange={e => { setZone(e.target.value); invalidate(); }} /></label>
      <p>존재하지 않는 현지 시각은 거절하며, 두 번 오는 시각은 첫 번째를 사용합니다.</p>
      <button disabled={busy || !local || !zone} onClick={async () => {
        const revision = generation; setBusy(true);
        try { const p = await request<MailSchedule>("/mail/schedule/preview", "POST", { local_datetime: local, timezone: zone }); if (revision === generation) setPreview(p); } catch (e) { fail(e); } finally { setBusy(false); }
      }}>정확한 시각 확인</button>
      {preview && <><p>{formatMailTime(preview)}</p><button disabled={busy} onClick={() => void save(preview)}>예약 시각 저장 · 발송은 다시 허락 필요</button></>}
      {action.schedule && <button disabled={busy} onClick={() => void save(null)}>예약 해제 · 지금 발송도 다시 허락 필요</button>}
      <button disabled={busy} onClick={() => { setOpen(false); editing(false); }}>시각 수정 취소</button>
    </div>}
  </section>;
}
