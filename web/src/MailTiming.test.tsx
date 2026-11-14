import {render,screen,act,fireEvent} from '@testing-library/react';
import {it,expect,vi} from 'vitest';
import {MailConnections} from './MailConnections';
import type {MailAction} from '../shared/api-types';
it('an unmounted A timing save cannot unlock approval while B timing is being edited',async()=>{
const make=(id:string):MailAction=>({id,version:1,revision:1,connection_id:'c',content:{from:'from@example.test',to:['to@example.test'],cc:[],bcc:[],subject:id,text:'synthetic',reply:null},digest:id,state:'pending_approval',expires_at:9999999999,message_id:id+'@example.test',provider_message_id:null,provider_thread_id:null,origin_thread_id:null,schedule:id==='A'?{due_at:1999999980,timezone:'UTC'}:undefined});
const a=make('A'),b=make('B');let release!:(r:Response)=>void;
vi.stubGlobal('fetch',vi.fn(async(path:string,_init:RequestInit={})=>{
 if(path.endsWith('/connections'))return Response.json({configured:true,connections:[{id:'c',email:'from@example.test',epoch:1,status:'active',compose_enabled:false}]});
 if(path.endsWith('/actions'))return Response.json({actions:[a,b]});
 if(path.endsWith('/actions/A/schedule'))return await new Promise<Response>(r=>release=r);
 throw Error('unexpected '+path);
}));
render(<MailConnections accountId='synthetic-owner' onExpired={vi.fn()} back={vi.fn()} originThreadId={null}/>);
fireEvent.click(await screen.findByRole('button',{name:'A · pending_approval'}));
fireEvent.click(screen.getByRole('button',{name:'예약 시각 설정·변경'}));
fireEvent.click(screen.getByRole('button',{name:'예약 해제 · 지금 발송도 다시 허락 필요'}));
fireEvent.click(screen.getByRole('button',{name:'B · pending_approval'}));
fireEvent.click(screen.getByRole('button',{name:'예약 시각 설정·변경'}));
fireEvent.change(screen.getByLabelText('발송할 날짜와 현지 시각'),{target:{value:'2030-01-01T10:00'}});
expect(screen.getByRole('button',{name:'허락하고 보내기'})).toBeDisabled();
fireEvent.click(screen.getByRole('button',{name:'B · pending_approval'}));
expect(screen.getByRole('button',{name:'허락하고 보내기'})).toBeDisabled();
await act(async()=>release(Response.json({...a,version:2,revision:2,schedule:null})));
expect(screen.getByLabelText('발송할 날짜와 현지 시각')).toHaveValue('2030-01-01T10:00');
expect(screen.getByRole('button',{name:'허락하고 보내기'})).toBeDisabled();
});
