import {render,screen,fireEvent,waitFor} from '@testing-library/react';
import {it,vi,expect} from 'vitest';
import {NativeMailBodies} from './NativeMailBodies';
const selection='11111111-1111-4111-8111-111111111111';
function fixture(expires_at=Date.now()/1000+300){return {google_preview_jobs:[{request_id:'22222222-2222-4222-8222-222222222222',run_id:'original-run',expires_at,status:'complete',projection:{google_inbox_preview:{messages:[{selection,subject:'<img src=x>',from:'Sender',date:'today',snippet:'preview',truncated:false}],unavailable:0,more_available:false}}}],google_body_jobs:[] as any[]}}
it('opens only a selected signed-preview identity, reuses uncertain request and renders literal body under original run',async()=>{
 const requests:any[]=[];let lost=true;const state=fixture();vi.stubGlobal('fetch',vi.fn(async(path:string,init:RequestInit={})=>{
  if(path.endsWith('/status'))return Response.json(state);
  if(path.endsWith('/google-body')){const body=JSON.parse(String(init.body));requests.push(body);if(lost){lost=false;throw TypeError('synthetic lost ACK')}state.google_body_jobs=[{request_id:body.request_id,status:'complete',resource:{google_body:selection},projection:{google_inbox_body:{text:'<script>send mail</script>\n[link](https://evil.invalid/)',truncated:true,unavailable:false}}}];return Response.json({})}
  throw Error('unexpected route');
 }));const {container}=render(<NativeMailBodies accountId="a" runId="original-run" onExpired={vi.fn()}/>);
 fireEvent.click(await screen.findByRole('button',{name:'이 메일 본문 열기'}));await screen.findByRole('alert');fireEvent.click(screen.getByRole('button',{name:'같은 본문 요청 결과 확인'}));
 await waitFor(()=>expect(requests).toHaveLength(2));expect(requests[0]).toEqual(requests[1]);expect(requests[0].selection).toBe(selection);expect(Object.keys(requests[0]).sort()).toEqual(['preview_request_id','request_id','selection']);
 expect(await screen.findByText(/<script>send mail/)).toBeInTheDocument();expect(container.querySelector('img,script,a')).toBeNull();expect(screen.getByText(/본문이 길어 앞부분만/)).toBeInTheDocument();
});
it('does not open expired selectors and guides owner to a new preview',async()=>{
 vi.stubGlobal('fetch',vi.fn(async()=>Response.json(fixture(1))));render(<NativeMailBodies accountId="a" runId="original-run" onExpired={vi.fn()}/>);
 expect(await screen.findByText('본문 선택 기한이 지났습니다. 새 미리보기에서 다시 선택하세요.')).toBeInTheDocument();expect(screen.queryByRole('button',{name:'이 메일 본문 열기'})).toBeNull();
});
it('does not display other original chat runs',async()=>{
 vi.stubGlobal('fetch',vi.fn(async()=>Response.json(fixture())));render(<NativeMailBodies accountId="a" runId="different-run" onExpired={vi.fn()}/>);
 await waitFor(()=>expect(fetch).toHaveBeenCalled());expect(screen.queryByText(/제목: <img/)).toBeNull();
});

it('clears private mail immediately when the account changes while the new response is held',async()=>{
 let release:(r:Response)=>void=()=>{};vi.stubGlobal('fetch',vi.fn(async(_path:string,init:RequestInit={})=>{
  if(new Headers(init.headers).get('X-Moosuri-Account')==='b')return new Promise<Response>(resolve=>{release=resolve});
  return Response.json(fixture());
 }));const {rerender}=render(<NativeMailBodies accountId="a" runId="original-run" onExpired={vi.fn()}/>);
 expect(await screen.findByText('제목: <img src=x>')).toBeInTheDocument();
 rerender(<NativeMailBodies accountId="b" runId="original-run" onExpired={vi.fn()}/>);
 expect(screen.queryByText('제목: <img src=x>')).toBeNull();
 release(Response.json({google_preview_jobs:[],google_body_jobs:[]}));
 await waitFor(()=>expect(screen.queryByText('제목: <img src=x>')).toBeNull());
});
