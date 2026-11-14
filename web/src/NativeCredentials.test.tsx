import {render,screen,fireEvent,waitFor} from '@testing-library/react';
import {it,vi,expect} from 'vitest';
import {NativeCredentials} from './NativeCredentials';
it('keeps original waiting request identity on uncertain response and never accepts a secret',async()=>{
 let requests:any[]=[];let lost=true;vi.stubGlobal('fetch',vi.fn(async(path:string,init:RequestInit={})=>{
  if(path.endsWith('/status'))return Response.json({devices:[],jobs:[]});
  if(path.endsWith('/github')){const body=JSON.parse(String(init.body));requests.push(body);if(lost){lost=false;throw TypeError('synthetic loss')}return Response.json({status:'waiting_for_credential'});}
  throw Error('unexpected route');
 }));
 render(<NativeCredentials accountId="a" onExpired={vi.fn()} back={vi.fn()}/>);
 fireEvent.change(screen.getByLabelText('소유자'),{target:{value:'owner'}});fireEvent.change(screen.getByLabelText('저장소'),{target:{value:'repo'}});fireEvent.click(screen.getByRole('button',{name:'저장소 메타데이터 읽기'}));
 await screen.findByRole('alert');fireEvent.click(screen.getByRole('button',{name:'같은 요청 결과 확인'}));await waitFor(()=>expect(requests).toHaveLength(2));expect(requests[0]).toEqual(requests[1]);expect(Object.keys(requests[0]).sort()).toEqual(['repository','request_id']);expect(screen.queryByLabelText(/password|token/i)).toBeNull();
});
it('shows disabled experimental availability honestly',async()=>{vi.stubGlobal('fetch',vi.fn(async()=>Response.json({}, {status:404})));render(<NativeCredentials accountId="a" onExpired={vi.fn()} back={vi.fn()}/>);expect(await screen.findByText('이 설치에서는 실험 기능이 꺼져 있습니다.')).toBeInTheDocument();expect(screen.queryByRole('button',{name:'저장소 메타데이터 읽기'})).toBeNull()});
it('reuses uncertain Gmail request and keeps old GitHub jobs distinct',async()=>{
 const requests:any[]=[];let lost=true;vi.stubGlobal('fetch',vi.fn(async(path:string,init:RequestInit={})=>{
  if(path.endsWith('/status'))return Response.json({devices:[],jobs:[{request_id:'old',repository:{owner:'owner',repository:'repo'},status:'complete',projection:null}],google_jobs:[{request_id:'google',status:'complete',projection:{google_inbox:{messages_total:21,messages_unread:7,threads_total:13,threads_unread:4}}}]});
  if(path.endsWith('/google-inbox')){const body=JSON.parse(String(init.body));requests.push(body);if(lost){lost=false;throw TypeError('synthetic loss')}return Response.json({status:'waiting_for_credential'})}throw Error('unexpected route');
 }));render(<NativeCredentials accountId="a" onExpired={vi.fn()} back={vi.fn()}/>);
 expect(await screen.findByText('owner/repo · complete')).toBeInTheDocument();expect(screen.getByText(/메시지 21개 · 읽지 않음 7개/)).toBeInTheDocument();
 fireEvent.click(screen.getByRole('button',{name:'Gmail INBOX 개수 읽기'}));await screen.findByRole('alert');fireEvent.click(screen.getByRole('button',{name:'같은 Gmail 요청 결과 확인'}));await waitFor(()=>expect(requests).toHaveLength(2));expect(requests[0]).toEqual(requests[1]);expect(Object.keys(requests[0])).toEqual(['request_id']);expect(screen.queryByLabelText(/password|token|Google.*계정/i)).toBeNull();
});
