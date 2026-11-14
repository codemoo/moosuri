import { useEffect, useRef, useState } from "react";
import type { AudioSettings, ProviderSummary } from "../shared/api-types";
import { useScope } from "./account-scope";
import { ApiFailure, isAborted } from "./api";
import { canonicalWav } from "./voice-audio";
import "./voice.css";
export function AudioSettingsPanel({ accountId, providers, onExpired }: {
    accountId: string;
    providers: ProviderSummary[];
    onExpired: () => void;
}) {
    const scope = useScope(accountId), operation = useRef<AbortController | null>(null);
    const [settings, setSettings] = useState<AudioSettings | null>(null), [selected, setSelected] = useState(""), [file, setFile] = useState<File | null>(null), [busy, setBusy] = useState(false), [error, setError] = useState(""), [notice, setNotice] = useState("");
    const providerKey = providers.map(p => `${p.id}:${p.revision}:${p.enabled}:${p.has_credential}`).join("|");
    const options = providers.filter(p => p.enabled && p.has_credential && p.config.kind === "open_ai");
    useEffect(() => { setBusy(false); const controller = new AbortController(); void scope.request<AudioSettings>("/audio/settings", "GET", undefined, controller.signal).then(value => { setSettings(value); setSelected(value.selection?.provider_id ?? ""); }).catch(e => { if (e instanceof ApiFailure && e.status === 401)
        onExpired(); }); return () => { controller.abort(); operation.current?.abort(); }; }, [scope, providerKey]);
    async function probe() { const provider = options.find(p => p.id === selected); if (!provider || !file)
        return; const controller = new AbortController(); operation.current?.abort(); operation.current = controller; setBusy(true); setError(""); setNotice(""); try {
        const wav = await canonicalWav(file, controller.signal);
        if (wav.size > 44 + 15 * 32000)
            throw new Error("확인용 녹음은 15초 이하로 선택해 주세요.");
        const value = await scope.audioUpload<AudioSettings>(crypto.randomUUID(), wav, () => { }, controller.signal, { provider_id: provider.id, provider_revision: provider.revision });
        setSettings(value);
        setNotice("음성 연결을 확인하고 저장했습니다.");
    }
    catch (e) {
        if (e instanceof ApiFailure && e.status === 401)
            onExpired();
        else if (!isAborted(e))
            setError("음성을 확인하지 못했습니다. 안내한 문장이 들리는 짧은 녹음과 API 연결을 확인해 주세요.");
    }
    finally {
        if (!controller.signal.aborted)
            setBusy(false);
    } }
    async function disable() { operation.current?.abort(); setBusy(true); setError(""); try {
        setSettings(await scope.request<AudioSettings>("/admin/audio/settings", "POST"));
        setNotice("음성 변환을 껐습니다.");
    }
    catch (e) {
        if (e instanceof ApiFailure && e.status === 401)
            onExpired();
        else
            setError("설정을 저장하지 못했습니다.");
    }
    finally {
        setBusy(false);
    } }
    return <section className="audio-settings" aria-labelledby="audio-settings-title"><h3 id="audio-settings-title">음성을 글로</h3><p>채팅 모델과 별도로 OpenAI API를 연결합니다. 구독 로그인은 음성 변환에 사용하지 않습니다.</p><p className="voice-help">{settings?.available ? "음성 연결이 준비되었습니다." : "음성 연결 확인이 필요합니다."} 모델: gpt-4o-mini-transcribe</p><label>OpenAI API 연결 <select value={selected} disabled={busy} onChange={e => setSelected(e.target.value)}><option value="">연결 선택</option>{options.map(p => <option key={p.id} value={p.id}>{p.name}</option>)}</select></label><p>아래 공개 문장이 들리는 15초 이하 녹음을 선택하세요. 녹음은 확인을 누를 때 OpenAI API로 전송됩니다.</p><blockquote lang="en">The blue bird sings beside the quiet river.</blockquote><label>확인용 녹음 <input type="file" accept="audio/*" disabled={busy} onChange={e => setFile(e.target.files?.[0] ?? null)}/></label><div className="voice-actions"><button type="button" className="primary" disabled={busy || !file || !selected} onClick={() => void probe()}>음성 연결 확인하고 저장</button>{settings?.selection && <button type="button" disabled={busy} onClick={() => void disable()}>음성 변환 끄기</button>}{busy && <button type="button" onClick={() => { operation.current?.abort(); setBusy(false); }}>취소</button>}</div>{busy && <p role="status">음성을 확인하고 있어요…</p>}{error && <p role="alert">{error}</p>}{notice && <p role="status">{notice}</p>}</section>;
}
