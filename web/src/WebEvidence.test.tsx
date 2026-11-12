import {render,screen} from "@testing-library/react";
import {expect,it} from "vitest";
import {WebEvidence} from "./WebEvidence";

it("links only recorded HTTP observations with accurate limits and safely replaces them on purge",()=>{
  const view=render(<WebEvidence observations={[{url:"https://example.com/evidence",title:"관측한 페이지",retrieved_at_unix_ms:1234,kind:"page",truncated:true},{url:"javascript:alert(1)",title:"fake",retrieved_at_unix_ms:1234,kind:"page",truncated:false}]}/>);
  const link=screen.getByRole("link",{name:/관측한 페이지/});expect(link).toHaveAttribute("href","https://example.com/evidence");expect(link).toHaveAttribute("rel","noopener noreferrer");expect(link.textContent).toContain("일부만 확인");expect(screen.queryByText("fake")).toBeNull();
  view.rerender(<WebEvidence observations={[]}/>);expect(screen.queryByRole("link")).toBeNull();
});
it("labels search snippets separately from page reading",()=>{
  render(<WebEvidence observations={[{url:"https://example.com/hit",title:"검색 결과",retrieved_at_unix_ms:1234,kind:"search_hit",truncated:false}]}/>);expect(screen.getByRole("link").textContent).toContain("검색 요약");
});
