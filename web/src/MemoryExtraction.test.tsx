import {act,fireEvent,render,screen,waitFor} from "@testing-library/react";
import {beforeEach,expect,it,vi} from "vitest";
import {MemoryExtraction} from "./MemoryExtraction";
import type {MemoryExtractionStatus} from "../shared/api-types";
const status=(extra:Partial<MemoryExtractionStatus>={}):MemoryExtractionStatus=>({revision:0,enabled:false,attribution_mode:"reported",reported_speaker:"",daily_calls:10,daily_input_bytes:24000,daily_output_tokens:4000,state:"idle",reason:null,saved_count:0,...extra});
const response=(v:unknown)=>new Response(JSON.stringify(v),{status:200,headers:{"content-type":"application/json"}});
beforeEach(()=>vi.restoreAllMocks());
function deferred(){let resolve!:(value:Response)=>void;const promise=new Promise<Response>(r=>resolve=r);return{promise,resolve}}
it("starts disabled and changes collection attribution only on explicit action, without backfill",async()=>{
 const calls:{path:string;init?:RequestInit}[]=[];vi.stubGlobal("fetch",vi.fn(async(path:string,init?:RequestInit)=>{calls.push({path,init});return response(status(init?.method==="PUT"?{revision:1,enabled:true,attribution_mode:"specified",reported_speaker:"Mina"}:{}))}));
 render(<MemoryExtraction accountId="a" collectionId="c" connected onExpired={vi.fn()}/>);await screen.findByRole("button",{name:"자동 기억 켜기"});expect(calls).toHaveLength(1);fireEvent.change(screen.getByLabelText("작성자 기준"),{target:{value:"specified"}});fireEvent.change(screen.getByLabelText("기억에 표시할 사람"),{target:{value:"Mina"}});fireEvent.click(screen.getByRole("button",{name:"자동 기억 켜기"}));await screen.findByRole("button",{name:"자동 기억 끄기"});expect(JSON.parse(String(calls[1].init?.body))).toMatchObject({enabled:true,reported_speaker:"Mina",attribution_mode:"specified",expected_revision:0});expect(calls.filter(c=>c.init?.method==="POST")).toHaveLength(0);
});
it("an existing source is processed only by its explicit pinned action",async()=>{
 const fetch=vi.fn(async(_p:string,i?:RequestInit)=>response(status({enabled:true,revision:1,state:i?.method==="POST"?"queued":"idle"})));vi.stubGlobal("fetch",fetch);render(<MemoryExtraction accountId="a" collectionId="c" connected onExpired={vi.fn()} source={{id:"source",revision:3}}/>);fireEvent.click(await screen.findByRole("button",{name:"이 자료에서 기억 찾기"}));await screen.findByRole("status");expect(fetch.mock.calls[1][0]).toBe("/api/v1/knowledge/sources/source/extraction");expect(JSON.parse(String(fetch.mock.calls[1][1]?.body))).toEqual({expected_revision:3});
});
it("late status from a prior account cannot fill another account's attribution",async()=>{
 let resolve!:(r:Response)=>void;const late=new Promise<Response>(r=>resolve=r);vi.stubGlobal("fetch",vi.fn(async(_p:string,i?:RequestInit)=>(i?.headers as Record<string,string>)["X-Moosuri-Account"]==="a"?late:response(status())));const props={collectionId:"c",connected:true,onExpired:vi.fn()};const view=render(<MemoryExtraction accountId="a" {...props}/>);view.rerender(<MemoryExtraction accountId="b" {...props}/>);await screen.findByRole("button",{name:"자동 기억 켜기"});await act(async()=>resolve(response(status({attribution_mode:"specified",reported_speaker:"Private former account"}))));await waitFor(()=>expect(screen.queryByDisplayValue("Private former account")).not.toBeInTheDocument());
});
it.each([false,true])("a pre-render mutation supersedes a delayed refresh when enabled=%s",async(enabled)=>{
 const read=deferred(),write=deferred();let reads=0;const onExpired=vi.fn();
 const fetch=vi.fn(async(_path:string,init?:RequestInit)=>init?.method==="GET"?(++reads===1?response(status({enabled})):read.promise):write.promise);vi.stubGlobal("fetch",fetch);
 render(<MemoryExtraction accountId="a" collectionId="c" connected onExpired={onExpired}/>);
 const toggle=await screen.findByRole("button",{name:enabled?"자동 기억 끄기":"자동 기억 켜기"});const refresh=screen.getByRole("button",{name:"결과 새로고침"});
 // Match the independent review repro: React has not rendered busy between handlers.
 act(()=>{refresh.click();toggle.click()});
 expect(fetch).toHaveBeenCalledTimes(3);expect(fetch.mock.calls[1][1]?.signal?.aborted).toBe(true);
 expect(JSON.parse(String(fetch.mock.calls[2][1]?.body))).toMatchObject({enabled:!enabled,expected_revision:0});
 // Even a stale failure cannot expire the account or clear the mutation's busy state.
 await act(async()=>read.resolve(new Response("{}",{status:401})));
 expect(onExpired).not.toHaveBeenCalled();expect(screen.queryByRole("alert")).not.toBeInTheDocument();expect(refresh).toBeDisabled();expect(toggle).toBeDisabled();
 await act(async()=>write.resolve(response(status({enabled:!enabled,revision:1}))));
 expect(screen.getByRole("button",{name:enabled?"자동 기억 켜기":"자동 기억 끄기"})).toBeEnabled();expect(screen.getByText(new RegExp(`현재 ${enabled?"꺼짐":"켜짐"}`))).toBeInTheDocument();
});
it.each([false,true])("an older successful refresh cannot replace accepted enabled=%s",async(enabled)=>{
 const read=deferred();let reads=0;
 const fetch=vi.fn(async(_path:string,init?:RequestInit)=>init?.method==="GET"?(++reads===1?response(status({enabled:!enabled})):read.promise):response(status({enabled,revision:1})));vi.stubGlobal("fetch",fetch);
 render(<MemoryExtraction accountId="a" collectionId="c" connected onExpired={vi.fn()}/>);
 const toggle=await screen.findByRole("button",{name:enabled?"자동 기억 켜기":"자동 기억 끄기"});const refresh=screen.getByRole("button",{name:"결과 새로고침"});
 act(()=>{refresh.click();toggle.click()});
 await screen.findByRole("button",{name:enabled?"자동 기억 끄기":"자동 기억 켜기"});
 await act(async()=>read.resolve(response(status({enabled:!enabled}))));
 expect(screen.getByText(new RegExp(`현재 ${enabled?"켜짐":"꺼짐"}`))).toBeInTheDocument();
 // The next explicit mutation must use the accepted revision, not the old read's.
 fireEvent.click(screen.getByRole("button",{name:enabled?"자동 기억 끄기":"자동 기억 켜기"}));
 expect(JSON.parse(String(fetch.mock.calls[3][1]?.body))).toMatchObject({expected_revision:1});
 await act(async()=>{});
});
it("serializes duplicate save and refresh handlers before busy is rendered",async()=>{
 const write=deferred();const fetch=vi.fn(async(_p:string,i?:RequestInit)=>i?.method==="PUT"?write.promise:response(status({enabled:true})));vi.stubGlobal("fetch",fetch);
 render(<MemoryExtraction accountId="a" collectionId="c" connected onExpired={vi.fn()}/>);
 const toggle=await screen.findByRole("button",{name:"자동 기억 끄기"});const save=screen.getByRole("button",{name:"작성자 기준 저장"});const refresh=screen.getByRole("button",{name:"결과 새로고침"});
 act(()=>{toggle.click();save.click();toggle.click();refresh.click()});
 expect(fetch).toHaveBeenCalledTimes(2);expect(JSON.parse(String(fetch.mock.calls[1][1]?.body))).toMatchObject({enabled:false});
 await act(async()=>write.resolve(response(status({enabled:false,revision:1}))));
 expect(screen.getByRole("button",{name:"자동 기억 켜기"})).toBeEnabled();
});
it("extract owns the pending action and supersedes refresh without dispatching twice",async()=>{
 const read=deferred(),write=deferred();let reads=0;
 const fetch=vi.fn(async(_p:string,i?:RequestInit)=>i?.method==="POST"?write.promise:(++reads===1?response(status({enabled:true})):read.promise));vi.stubGlobal("fetch",fetch);
 render(<MemoryExtraction accountId="a" collectionId="c" connected onExpired={vi.fn()} source={{id:"source",revision:3}}/>);
 const extract=await screen.findByRole("button",{name:"이 자료에서 기억 찾기"});const refresh=screen.getByRole("button",{name:"결과 새로고침"});
 act(()=>{refresh.click();extract.click();extract.click();refresh.click()});
 expect(fetch).toHaveBeenCalledTimes(3);expect(fetch.mock.calls[1][1]?.signal?.aborted).toBe(true);expect(JSON.parse(String(fetch.mock.calls[2][1]?.body))).toEqual({expected_revision:3});
 await act(async()=>read.resolve(response(status({enabled:false}))));expect(extract).toBeDisabled();expect(refresh).toBeDisabled();
 await act(async()=>write.resolve(response(status({enabled:true,state:"queued"}))));
 expect(await screen.findByRole("status")).toHaveTextContent("자료를 확인하고 있습니다");expect(extract).toBeEnabled();expect(screen.getByText(/현재 켜짐/)).toBeInTheDocument();
});
it.each([200,401])("a prior account's mutation response (%s) cannot update or expire the new account",async(code)=>{
 const write=deferred();const onExpired=vi.fn();const fetch=vi.fn(async(_p:string,i?:RequestInit)=>i?.method==="PUT"?write.promise:response(status()));vi.stubGlobal("fetch",fetch);
 const props={collectionId:"c",connected:true,onExpired};const view=render(<MemoryExtraction accountId="a" {...props}/>);fireEvent.click(await screen.findByRole("button",{name:"자동 기억 켜기"}));
 view.rerender(<MemoryExtraction accountId="b" {...props}/>);await screen.findByRole("button",{name:"자동 기억 켜기"});
 await act(async()=>write.resolve(code===200?response(status({enabled:true,revision:1})):new Response("{}",{status:401})));
 expect(fetch.mock.calls[1][1]?.signal?.aborted).toBe(true);expect(screen.getByRole("button",{name:"자동 기억 켜기"})).toBeEnabled();expect(screen.getByText(/현재 꺼짐/)).toBeInTheDocument();expect(onExpired).not.toHaveBeenCalled();expect(screen.queryByRole("alert")).not.toBeInTheDocument();
});
it("a source revision change fences the prior extraction and pins the next request",async()=>{
 const write=deferred();const fetch=vi.fn(async(_p:string,i?:RequestInit)=>i?.method==="POST"?write.promise:response(status({enabled:true})));vi.stubGlobal("fetch",fetch);
 const props={accountId:"a",collectionId:"c",connected:true,onExpired:vi.fn()};const view=render(<MemoryExtraction {...props} source={{id:"source",revision:3}}/>);fireEvent.click(await screen.findByRole("button",{name:"이 자료에서 기억 찾기"}));
 view.rerender(<MemoryExtraction {...props} source={{id:"source",revision:4}}/>);await screen.findByRole("button",{name:"이 자료에서 기억 찾기"});
 await act(async()=>write.resolve(response(status({enabled:true,state:"completed",saved_count:4}))));
 expect(fetch.mock.calls[1][1]?.signal?.aborted).toBe(true);expect(screen.queryByRole("status")).not.toBeInTheDocument();
 fireEvent.click(screen.getByRole("button",{name:"이 자료에서 기억 찾기"}));expect(JSON.parse(String(fetch.mock.calls[3][1]?.body))).toEqual({expected_revision:4});await act(async()=>{});
});
