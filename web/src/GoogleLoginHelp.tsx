import { useState } from "react";
import { VaultLaunch } from "./VaultLaunch";
import "./google-login-help.css";

// Password entry and decrypted items stay in the separate trusted vault client.
// This assistant component intentionally never receives login values or messages
// from the vault. A user's choice here grants no provider or send permission.
export function GoogleLoginHelp({ accountId }: { accountId: string }) {
  const [choice, setChoice] = useState<"saved" | "save" | null>(null);
  return <div className="google-login-help">
    <p><strong>Google 로그인 준비 · 선택 사항</strong></p>
    <p>이 기기의 Google 로그인이나 Google·기기 암호 관리자를 그대로 사용할 수 있어요. 볼트 저장은 Gmail 연결의 필수 단계가 아닙니다.</p>
    <div className="google-login-choices">
      <button type="button" aria-pressed={choice === "saved"} onClick={() => setChoice(choice === "saved" ? null : "saved")}>저장된 Google 로그인 사용</button>
      <button type="button" aria-pressed={choice === "save"} onClick={() => setChoice(choice === "save" ? null : "save")}>ID·비밀번호를 내 볼트에 보관</button>
    </div>
    {choice && <div className="google-login-instructions">
      {choice === "saved" ? <p>암호 관리자에 저장된 계정은 다음 Google 공식 화면에서 선택하세요. 내 볼트에 저장했다면 볼트를 열어 Google 항목을 찾고, 필요한 값만 직접 복사해 공식 화면에서 사용하세요.</p> : <ol>
        <li>별도 볼트를 열고 잠금을 해제하세요. 처음이라면 볼트 비밀번호를 정하고 복구 키를 보관·확인하세요.</li>
        <li>‘새 항목’ → ‘로그인’을 선택하세요. 이름은 Google, 사용자 이름은 Google ID, 주소는 <code>https://accounts.google.com</code>으로 입력하고 비밀번호를 저장하세요.</li>
        <li>저장 결과를 볼트에서 확인한 뒤 이 화면으로 돌아와 Google 연결을 진행하세요. 저장만으로 Gmail이 연결되지는 않습니다.</li>
      </ol>}
      <VaultLaunch accountId={accountId} label="내 Google 로그인 볼트 열기 ↗" unavailable="이 설치에서 별도 볼트를 열 수 없습니다. Google·기기 암호 관리자로 바로 연결하거나 설치자에게 볼트 연결을 확인해 달라고 요청하세요." />
      <p className="fine">ID·비밀번호는 채팅에 쓰지 마세요. 비밀번호 입력·저장은 별도 볼트에서만 하며, 로그인·2단계 인증·권한 동의는 accounts.google.com에서 직접 완료하세요. 볼트 값이 채팅으로 전달되지는 않습니다.</p>
    </div>}
  </div>;
}
