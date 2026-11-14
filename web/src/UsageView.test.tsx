import {render,screen} from "@testing-library/react";
import {it,expect,vi,afterEach} from "vitest";
import {UsageView} from "./UsageView";
afterEach(()=>vi.unstubAllGlobals());
it("shows observed samples and failed/unknown denominators without inventing missing first body time",async()=>{
 const missing={samples:0,missing:3,p50_ms:null,p95_ms:null};
 const timings={runs:3,attempts:3,unmeasured_runs:1,not_dispatched:1,truncated:false,outcomes:{completed:1,failed:1,outcome_unknown:1},queue:{samples:2,missing:1,p50_ms:20,p95_ms:30},context:missing,first_dispatch:missing,model:missing,tool:missing,first_final_body:missing,terminal:missing};
 vi.stubGlobal("fetch",vi.fn(async()=>new Response(JSON.stringify({dispatched_calls:2,unreported_calls:1,reported_input_tokens:7,reported_output_tokens:3,reported_cache_read_tokens:0,reported_cache_write_tokens:0,timings}))));
 render(<UsageView accountId="synthetic-owner" onExpired={vi.fn()}/>);
 expect(await screen.findByText("보관 중인 최근 서버 응답 시간")).toBeInTheDocument();
 expect(screen.getByText(/완료 1 · 실패 1 · 취소 0 · 결과 불명 1/)).toBeInTheDocument();
 expect(screen.getAllByText(/중앙값 측정값 없음 · 95백분위 측정값 없음 · 표본 0 · 누락 3/)).toHaveLength(6);
 expect(screen.getByText(/과거 시간 정보 없음 1개/)).toBeInTheDocument();
});
