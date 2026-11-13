import {act,fireEvent,render,screen,waitFor} from "@testing-library/react";
import {beforeEach,it,expect,vi} from "vitest";
import {useState} from "react";
import {PrivateFile} from "./PrivateFiles";
import {useMediaUploads,MediaLibraryView} from "./Media";
import {prepareImage} from "./media-prepare";
import type {MediaSummary} from "../shared/api-types";
vi.mock("./media-prepare",async importOriginal=>({...await importOriginal<typeof import("./media-prepare")>(),prepareImage:vi.fn()}));
const file:MediaSummary={id:"11111111-1111-4111-8111-111111111111",revision:1,digest:"a".repeat(64),filename:"<script>.md",media_type:"text/markdown",state:"ready",width:0,height:0,bytes:17,created_at:1};
class Upload{
 static jobs:Upload[]=[];upload:{onprogress:((event:ProgressEvent)=>void)|null}={onprogress:null};onload:(()=>void)|null=null;onerror:(()=>void)|null=null;onabort:(()=>void)|null=null;status=200;responseText=JSON.stringify(file);url="";headers:Record<string,string>={};withCredentials=false;body?:Blob;
 constructor(){Upload.jobs.push(this);}open(_method:string,url:string){this.url=url;}setRequestHeader(k:string,v:string){this.headers[k]=v;}send(blob:Blob){this.body=blob;}abort(){this.onabort?.();}finish(){this.onload?.();}
}
beforeEach(()=>{Upload.jobs=[];vi.stubGlobal("XMLHttpRequest",Upload);vi.stubGlobal("fetch",vi.fn());vi.stubGlobal("URL",Object.assign(URL,{createObjectURL:vi.fn(()=>"blob:synthetic-file"),revokeObjectURL:vi.fn()}));vi.mocked(prepareImage).mockReset();});
function Harness({account="account-a"}:{account?:string}){const uploads=useMediaUploads(account,"thread-a");const[text,setText]=useState("");return <>{uploads.view}<input aria-label="Message" value={text} onChange={e=>setText(e.target.value)}/><button onClick={()=>uploads.sent(uploads.readySelections)}>Send {uploads.ready.length} ready attachments</button><output>{JSON.stringify(uploads.ready)}</output></>;}
it("uploads a document independently of pending image preparation and sending text retains pending file",async()=>{
 vi.mocked(prepareImage).mockImplementation(()=>new Promise(()=>{}));render(<Harness/>);
 fireEvent.change(screen.getByLabelText("이미지 첨부"),{target:{files:[new File(["image"],"photo.png",{type:"image/png"})]}});
 fireEvent.change(screen.getByLabelText("파일 첨부"),{target:{files:[new File(["# Synthetic\n37"],"report.md",{type:"text/markdown"})]}});
 await waitFor(()=>expect(Upload.jobs).toHaveLength(1));expect(Upload.jobs[0].url).toContain("/api/v1/files/");expect(Upload.jobs[0].url).toContain("filename=report.md");expect(Upload.jobs[0].headers["X-Moosuri-Account"]).toBe("account-a");
 fireEvent.change(screen.getByLabelText("Message"),{target:{value:"Draft while uploading"}});fireEvent.click(screen.getByText("Send 0 ready attachments"));expect(screen.getByText("report.md")).toBeInTheDocument();expect(screen.getByLabelText("Message")).toHaveValue("Draft while uploading");
 act(()=>Upload.jobs[0].upload.onprogress?.({lengthComputable:true,loaded:3,total:10} as ProgressEvent));expect(screen.getByText("업로드 30%")).toBeInTheDocument();
 await act(async()=>Upload.jobs[0].finish());expect(screen.getByText("Send 1 ready attachments")).toBeInTheDocument();fireEvent.click(screen.getByText("Send 1 ready attachments"));expect(screen.queryByText("report.md")).not.toBeInTheDocument();expect(screen.getByText("이미지 준비 중")).toBeInTheDocument();
});
it("keeps original id and bytes through unknown ACK retry, cancellation fences a late ACK",async()=>{
 vi.mocked(fetch).mockResolvedValue(new Response(null,{status:204}));render(<Harness/>);const original=new File(["amount,37"],"sample.csv");fireEvent.change(screen.getByLabelText("파일 첨부"),{target:{files:[original]}});await waitFor(()=>expect(Upload.jobs).toHaveLength(1));const first=Upload.jobs[0];act(()=>first.onerror?.());fireEvent.click(await screen.findByText("같은 파일 재시도"));await waitFor(()=>expect(Upload.jobs).toHaveLength(2));expect(Upload.jobs[1].url).toBe(first.url);expect(Upload.jobs[1].body).toBe(first.body);fireEvent.click(screen.getByText("취소"));await act(async()=>Upload.jobs[1].finish());expect(screen.getByText("Send 0 ready attachments")).toBeInTheDocument();expect(screen.queryByText("sample.csv")).not.toBeInTheDocument();expect(fetch).toHaveBeenCalledWith(expect.stringContaining("/api/v1/media/"),expect.objectContaining({method:"DELETE"}));
});
it("rejects unsupported formats and oversize text/PDF before upload",async()=>{
 render(<Harness/>);for(const item of [new File(["binary"],"report.docx"),new File(["x".repeat(24577)],"huge.txt"),new File([new Uint8Array(2*1024*1024+1)],"huge.pdf")]){fireEvent.change(screen.getByLabelText("파일 첨부"),{target:{files:[item]}});}await screen.findAllByText(/텍스트 파일은 24KiB 이하/);expect(Upload.jobs).toHaveLength(0);
});

it("uploads PDF as an independent original and explains an image-only PDF without erasing the draft",async()=>{
 render(<Harness/>);const pdf=new File(["%PDF-1.5\nsynthetic transport"],"receipt.pdf",{type:"application/pdf"});
 fireEvent.change(screen.getByLabelText("파일 첨부"),{target:{files:[pdf]}});await waitFor(()=>expect(Upload.jobs).toHaveLength(1));
 expect(Upload.jobs[0].body).toBe(pdf);expect(Upload.jobs[0].url).toContain("filename=receipt.pdf");expect(prepareImage).not.toHaveBeenCalled();
 fireEvent.change(screen.getByLabelText("Message"),{target:{value:"이 영수증 정리해줘"}});Upload.jobs[0].status=400;Upload.jobs[0].responseText=JSON.stringify({code:"pdf_no_text"});
 await act(async()=>Upload.jobs[0].finish());await screen.findByText(/스캔본이라면 필요한 페이지를 이미지로 첨부/);expect(screen.getByLabelText("Message")).toHaveValue("이 영수증 정리해줘");
 fireEvent.click(screen.getByText("같은 파일 재시도"));await waitFor(()=>expect(Upload.jobs).toHaveLength(2));expect(Upload.jobs[1].body).toBe(pdf);expect(Upload.jobs[1].url).toBe(Upload.jobs[0].url);
});

it("labels PDF text extraction honestly and renders document markup as inert text",async()=>{
 vi.mocked(fetch).mockResolvedValue(new Response(JSON.stringify("[Page 1]\n<script>source only</script>"),{headers:{"content-type":"application/json"}}));
 const view=render(<PrivateFile accountId="account-a" media={{...file,filename:"receipt.pdf",media_type:"application/pdf"}}/>);
 fireEvent.click(screen.getByText("텍스트 미리보기"));await screen.findByText(/PDF에서 추출한 텍스트/);expect(view.container.querySelector("script")).toBeNull();expect(screen.getByText(/source only/)).toBeInTheDocument();
});
it("renders markup as text and deletion blocks a pending original download and late preview",async()=>{
 let previewResolve!:(v:Response)=>void;let downloadResolve!:(v:Response)=>void;vi.mocked(fetch).mockImplementation((url)=>String(url).endsWith("/preview")?new Promise(r=>{previewResolve=r;}):new Promise(r=>{downloadResolve=r;}));const view=render(<PrivateFile accountId="account-a" media={file}/>);
 expect(screen.getByText("<script>.md")).toBeInTheDocument();fireEvent.click(screen.getByText("원문 미리보기"));fireEvent.click(screen.getByText("원본 다운로드"));act(()=>window.dispatchEvent(new CustomEvent("moosuri-media-deleted",{detail:file.id})));
 await act(async()=>{previewResolve(new Response(JSON.stringify("<script>bad()</script>"),{headers:{"content-type":"application/json"}}));downloadResolve(new Response("original",{headers:{"content-type":"application/octet-stream"}}));});expect(screen.queryByText("<script>bad()</script>")).not.toBeInTheDocument();expect(URL.createObjectURL).not.toHaveBeenCalled();expect(view.container.querySelector("script")).toBeNull();expect(screen.getByText(/삭제된 파일/)).toBeInTheDocument();
});
it("escapes active-looking source text and closes pending data on account switch",async()=>{
 vi.mocked(fetch).mockResolvedValue(new Response(JSON.stringify("<img src=x onerror=attack()>\n# Plain Markdown"),{headers:{"content-type":"application/json"}}));const view=render(<PrivateFile accountId="account-a" media={file}/>);fireEvent.click(screen.getByText("원문 미리보기"));await screen.findByText(/<img src=x onerror=attack\(\)>/);expect(view.container.querySelector("img")).toBeNull();expect(view.container.querySelector("h1")).toBeNull();
 let finish!:(v:Response)=>void;vi.mocked(fetch).mockImplementation(()=>new Promise(r=>{finish=r;}));fireEvent.click(screen.getByText("원문 미리보기"));view.rerender(<PrivateFile accountId="account-b" media={{...file,id:"second-file"}}/>);await act(async()=>finish(new Response(JSON.stringify("old account secret"),{headers:{"content-type":"application/json"}})));expect(screen.queryByText("old account secret")).not.toBeInTheDocument();expect(screen.queryByText(/Plain Markdown/)).not.toBeInTheDocument();
});
it("account lifetime closes a document upload even when the server returns after switching",async()=>{
 const view=render(<Harness key="account-a"/>);fireEvent.change(screen.getByLabelText("파일 첨부"),{target:{files:[new File(["Synthetic"],"old.txt")]}});await waitFor(()=>expect(Upload.jobs).toHaveLength(1));view.rerender(<Harness account="account-b" key="account-b"/>);await act(async()=>Upload.jobs[0].finish());expect(screen.getByText("Send 0 ready attachments")).toBeInTheDocument();expect(screen.queryByText("old.txt")).not.toBeInTheDocument();
});

it("recovers a reopened pending library upload with its original UUID and selects that exact ready source",async()=>{
 let ready=false;const onSelect=vi.fn();const original={...file,filename:"original.txt"};vi.mocked(fetch).mockImplementation(()=>Promise.resolve(new Response(JSON.stringify({items:[{...original,state:ready?"ready":"retry_required"}],used_bytes:ready?34:0,quota_bytes:67108864}),{headers:{"content-type":"application/json"}})));
 render(<MediaLibraryView accountId="account-a" onSelect={onSelect}/>);fireEvent.click(screen.getByText("새로고침"));const picker=await screen.findByLabelText("원본 다시 선택: original.txt");fireEvent.change(picker,{target:{files:[new File(["Synthetic original"],"original.txt")]}});await waitFor(()=>expect(Upload.jobs).toHaveLength(1));expect(Upload.jobs[0].url).toBe(`/api/v1/files/${file.id}?filename=original.txt`);expect(Upload.jobs[0].headers["X-Moosuri-Account"]).toBe("account-a");ready=true;await act(async()=>Upload.jobs[0].finish());fireEvent.click(await screen.findByRole("button",{name:"대화에 첨부"}));expect(onSelect).toHaveBeenCalledWith(expect.objectContaining({id:file.id,revision:1,state:"ready"}));
});

it("deleting a pending library recovery aborts transport and rejects its late completion",async()=>{
 let deleted=false;vi.mocked(fetch).mockImplementation((_url,init)=>{if(init?.method==="DELETE"){deleted=true;return Promise.resolve(new Response(null,{status:204}));}return Promise.resolve(new Response(JSON.stringify({items:deleted?[]:[{...file,filename:"original.txt",state:"retry_required"}],used_bytes:0,quota_bytes:67108864}),{headers:{"content-type":"application/json"}}));});
 render(<MediaLibraryView accountId="account-a" onSelect={vi.fn()}/>);fireEvent.click(screen.getByText("새로고침"));fireEvent.change(await screen.findByLabelText("원본 다시 선택: original.txt"),{target:{files:[new File(["Synthetic original"],"original.txt")]}});await waitFor(()=>expect(Upload.jobs).toHaveLength(1));const abort=vi.spyOn(Upload.jobs[0],"abort");fireEvent.click(screen.getByText("파일 삭제"));await screen.findByText("보관한 첨부가 없습니다.");expect(abort).toHaveBeenCalled();await act(async()=>Upload.jobs[0].finish());expect(screen.queryByRole("button",{name:"대화에 첨부"})).not.toBeInTheDocument();
});
