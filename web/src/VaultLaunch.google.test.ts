// @vitest-environment jsdom
import {afterEach,expect,it,vi} from "vitest";
import {lockVaultWindows,postVaultForm} from "./VaultLaunch";

afterEach(()=>{lockVaultWindows();vi.restoreAllMocks()});
it("accepts a different selected email only from the exact vault popup and keeps an uncertain handoff open",async()=>{
  vi.spyOn(HTMLFormElement.prototype,"submit").mockImplementation(()=>{});
  const popup={closed:false,close:vi.fn(),postMessage:vi.fn()} as unknown as Window;
  const receive=vi.fn().mockRejectedValue(new TypeError("lost response"));
  const job=crypto.randomUUID(), origin="https://vault.example.test";
  postVaultForm(popup,"vault",{token:"a".repeat(64),vault_origin:origin},"member","owner@example.test",{job,receive});
  const data={kind:"moosuri-vault-kiri-sealed",account:"member",job,email:"selected@example.test",envelope:{body:"sealed"}};
  window.dispatchEvent(new MessageEvent("message",{origin:"https://other.test",source:popup,data}));
  window.dispatchEvent(new MessageEvent("message",{origin,source:window,data}));
  expect(receive).not.toHaveBeenCalled();
  window.dispatchEvent(new MessageEvent("message",{origin,source:popup,data}));
  await Promise.resolve();
  expect(receive).toHaveBeenCalledExactlyOnceWith(data.envelope,"selected@example.test");
  expect(popup.close).not.toHaveBeenCalled();
});
