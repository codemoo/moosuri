import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { CalendarEvidence } from "./CalendarEvidence";

const base = {action_id:"synthetic-action",operation:"delete",event_id:"event1",provider_etag:null};
describe("trusted Calendar receipts",()=>{
  it("keeps an unknown effect distinct from applied, without a resend control",()=>{
    render(<CalendarEvidence receipt={{...base,state:"outcome_unknown",message:"Outcome unknown; lookup only, no resend."}}/>);
    expect(screen.getByText(/결과 확인 필요/)).toBeTruthy();
    expect(screen.queryByRole("button")).toBeNull();
    expect(screen.getByRole("link").getAttribute("href")).toBe("/settings/calendar");
    expect(screen.getByText("synthetic-action")).toBeTruthy();
  });
  it("states current absence without causal deletion success",()=>{
    render(<CalendarEvidence receipt={{...base,state:"observed_absent",message:"Current absence observed; causality unknown."}}/>);
    expect(screen.getByText(/현재 없음/)).toBeTruthy();
    expect(screen.queryByText(/적용됨/)).toBeNull();
  });
});
