import type {Command} from "./types";
export class VaultFailure extends Error{constructor(code:string,readonly uncertain=false){super(code)}}
/** One account generation owns every callback; close rejects before terminate. */
export class WorkerClient{
  private sequence=0;private active=true;
  private pending=new Map<number,{resolve:(v:unknown)=>void;reject:(e:Error)=>void}>();
  constructor(private worker:Worker,private epoch:number,private onLock:(reason:string)=>void){
    worker.onmessage=event=>{
      if(!this.active)return;
      const result=event.data;
      if(result.kind==="locked"){this.close();this.onLock(result.reason);return}
      if(result.epoch!==epoch||!Number.isSafeInteger(result.id))return;
      const p=this.pending.get(result.id);if(!p)return;this.pending.delete(result.id);
      if(result.ok)p.resolve(result.value);else p.reject(new VaultFailure(result.error,result.uncertain===true));
    };
    worker.onerror=()=>{if(this.active){this.close();this.onLock("vault_unavailable")}};
  }
  request<T>(command:Command):Promise<T>{
    if(!this.active)return Promise.reject(new VaultFailure("vault_locked"));
    if(this.pending.size>=8)return Promise.reject(new VaultFailure("vault_busy"));
    const id=++this.sequence;return new Promise<T>((resolve,reject)=>{
      this.pending.set(id,{resolve:v=>resolve(v as T),reject});this.worker.postMessage({id,epoch:this.epoch,command});
    });
  }
  close(){
    if(!this.active)return;this.active=false;for(const p of this.pending.values())p.reject(new VaultFailure("vault_locked"));this.pending.clear();
    this.worker.postMessage({id:++this.sequence,epoch:this.epoch,command:{op:"close"}});this.worker.terminate();
  }
}
