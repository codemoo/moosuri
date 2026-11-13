import {expect,it,vi} from "vitest";
import {WorkerClient} from "./worker-client";
class FakeWorker{
  onmessage:((e:{data:unknown})=>void)|null=null;onerror:(()=>void)|null=null;
  sent:unknown[]=[];terminate=vi.fn();postMessage(value:unknown){this.sent.push(value)}
  reply(value:unknown){this.onmessage?.({data:value})}
}
it("close rejects owned work and ignores delayed plaintext from an older account generation",async()=>{
  const worker=new FakeWorker(),lock=vi.fn();const client=new WorkerClient(worker as unknown as Worker,7,lock);
  const pending=client.request({op:"reveal",item:"synthetic",field:"secret"});const rejected=expect(pending).rejects.toThrow("vault_locked");
  client.close();worker.reply({id:1,epoch:7,ok:true,value:"SYNTHETIC OLD ACCOUNT CANARY"});await rejected;
  expect(worker.terminate).toHaveBeenCalledTimes(1);expect(lock).not.toHaveBeenCalled();
});
it("a foreign callback cannot resolve selected-field work and a lock closes every pending request",async()=>{
  const worker=new FakeWorker(),lock=vi.fn();const client=new WorkerClient(worker as unknown as Worker,11,lock);let resolved=false;
  const first=client.request<string>({op:"reveal"}).then(v=>{resolved=true;return v});
  worker.reply({id:1,epoch:10,ok:true,value:"wrong generation"});await Promise.resolve();expect(resolved).toBe(false);
  worker.reply({id:1,epoch:11,ok:true,value:"selected value"});await expect(first).resolves.toBe("selected value");
  const pending=client.request({op:"list"});const rejected=expect(pending).rejects.toThrow("vault_locked");worker.reply({kind:"locked",reason:"vault_unauthorized"});await rejected;expect(lock).toHaveBeenCalledWith("vault_unauthorized");
});
