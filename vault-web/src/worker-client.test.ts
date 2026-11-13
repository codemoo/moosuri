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
it.each(["archive_file","archive_select","csv_next","publish"])("lock terminates %s work and disposes a late archive/import response",async op=>{
  const worker=new FakeWorker(),client=new WorkerClient(worker as unknown as Worker,17,vi.fn());
  const pending=client.request({op});const rejected=expect(pending).rejects.toThrow("vault_locked");
  client.close();worker.reply({id:1,epoch:17,ok:true,value:{eligible:2,rows:[{row:1,status:"new"}]}});
  await rejected;expect(worker.terminate).toHaveBeenCalledTimes(1);
  await expect(client.request({op:"csv_next"})).rejects.toThrow("vault_locked");
});
it("partial import ACK failure preserves uncertainty for exact reconciliation",async()=>{
  const worker=new FakeWorker(),client=new WorkerClient(worker as unknown as Worker,19,vi.fn());
  const pending=client.request({op:"publish"});const rejected=expect(pending).rejects.toMatchObject({message:"vault_unavailable",uncertain:true});
  worker.reply({id:1,epoch:19,ok:false,error:"vault_unavailable",uncertain:true});await rejected;
  const reconciled=client.request({op:"reconcile"});worker.reply({id:2,epoch:19,ok:true,value:{saved:true}});
  await expect(reconciled).resolves.toEqual({saved:true});client.close();
});
