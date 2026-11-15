import {fireEvent,render,screen,waitFor} from "@testing-library/react";
import {expect,it,vi} from "vitest";
import {CompoundRecipeEditor} from "./CompoundRecipeEditor";
import type {CommunitySkillCard} from "../shared/api-types";
it("loads only structural plan and routes edits through parent CAS ownership",async()=>{
 const calculation={origins:["source","current"],graph:{input_count:2,steps:[{operation:"product",operands:[{kind:"input",index:0},{kind:"input",index:1}]}]}};
 const request=vi.fn(async(_path:string)=>Response.json({schema_version:1,revision:2,calculation}));vi.stubGlobal("fetch",request);
 const edit=vi.fn(async()=>true);
 render(<CompoundRecipeEditor accountId="owner-synthetic" card={{id:"private:synthetic",revision:7} as CommunitySkillCard} onExpired={vi.fn()} edit={edit}/>);
 fireEvent.click(screen.getByText("계산 구조 편집"));
 const input=await screen.findByRole("textbox",{name:"계산 구조"});
 expect(JSON.parse((input as HTMLTextAreaElement).value)).toEqual(calculation);
 fireEvent.click(screen.getByText("새 버전으로 저장"));
 await waitFor(()=>expect(edit).toHaveBeenCalledWith(calculation));
 expect(request).toHaveBeenCalledTimes(1);
 expect(String(request.mock.calls[0]?.[0])).toContain("/community-skills/private%3Asynthetic/recipe");
 vi.unstubAllGlobals();
});
