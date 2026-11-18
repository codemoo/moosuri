import { useEffect, useState } from "react";
import { useScope } from "./account-scope";
import "./google-login-help.css";

export function GwsInboxHelp({ accountId }: { accountId: string }) {
  const scope = useScope(accountId);
  const [status, setStatus] = useState<{configured: boolean; mailbox?: string} | null>(null);
  const [failed, setFailed] = useState(false);
  const [refresh, setRefresh] = useState(0);
  useEffect(() => {
    let live = true;
    setStatus(null); setFailed(false);
    void scope.request<{configured: boolean; mailbox?: string}>("/mail/gws-inbox")
      .then(value => { if (live) setStatus(value); })
      .catch(() => { if (live) setFailed(true); });
    return () => { live = false; };
  }, [scope, refresh]);
  return <section className="gws-inbox-help" aria-label="기기 Gmail 연결">
    <h4>기기의 Google 로그인으로 최근 받은메일 읽기</h4>
    <p>현재는 운영자가 지정한 Mac의 GWS 연결로 최근 받은메일만 읽을 수 있어요. 메일 발송·전체 검색·첨부 읽기는 이 연결에서 지원하지 않습니다.</p>
    <p role="status">{failed ? "기기 연결 설정을 확인하지 못했습니다. 기기가 오프라인이거나 연결이 준비되지 않았을 수 있어요." : !status ? "기기 연결 설정 확인 중…" : status.configured ? `운영자 연결 설정 있음${status.mailbox ? ` · ${status.mailbox}` : ""}. 실제 메일 조회 시 기기 연결과 Google 인증을 확인합니다.` : "아직 이 사용자의 기기 Gmail 연결이 설정되지 않았습니다."}</p>
    <button type="button" disabled={!status && !failed} onClick={() => setRefresh(value => value + 1)}>기기 연결 설정 새로고침</button>
    <details><summary>Mac에서 GWS 연결 준비하기</summary>
      <ol>
        <li>설치자와 함께 사용할 Mac에 <a href="https://github.com/googleworkspace/cli" target="_blank" rel="noreferrer">Google Workspace CLI(gws)</a>를 준비하세요.</li>
        <li>그 Mac의 터미널에서 <code>gws auth login --readonly --services gmail</code>을 실행하세요. 열린 Google 공식 화면에서 본인 계정 로그인·2단계 인증·읽기 권한 동의를 완료하세요.</li>
        <li>설치자가 이 사용자와 그 기기의 GWS 브리지를 연결해야 합니다. Mac이 켜져 있고 브리지에 접속할 수 있어야 해요. Google 로그인만으로 무수리 연결이 완료되지는 않습니다.</li>
      </ol>
      <p>준비 후 대화로 돌아가 최근 받은메일을 요청하세요. 채팅에 Google ID·비밀번호·인증 토큰을 입력하지 마세요.</p>
    </details>
  </section>;
}
