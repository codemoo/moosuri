import {describe,expect,it} from "vitest";
import {MAX_ITEMS,MAX_TERMS,formatMiB,initialSelection,parseTerms,scopeLabel,selectionState,selector,workerSelector} from "./import-filter";

describe("selective import helpers (synthetic data only)",()=>{
  it("parses a short generic any-of list, trimming and de-duplicating case-insensitively",()=>{
    expect(parseTerms(" Alpha-One, beta ;\nALPHA-one\n")).toEqual({terms:["Alpha-One","beta"],error:""});
    expect(parseTerms("Ünique")).toEqual({terms:["Ünique"],error:""});
  });
  it("refuses empty, one-character, oversized and too many terms",()=>{
    for(const [text,error] of [["","empty"],[" , ;\n","empty"],["ok,x","short"],["x","short"],["a".repeat(129),"long"],[Array.from({length:MAX_TERMS+1},(_,n)=>`term-${n}`).join(","),"many"]] as const)
      expect(parseTerms(text).error).toBe(error);
  });
  it("never turns a blank selector into an all-items import",()=>{
    expect(selector("all","","")).toEqual({ok:true,value:{username:null,usernames:null}});
    expect(selector("exact","   ","ignored").ok).toBe(false);
    expect(selector("any","ignored","").ok).toBe(false);
    expect(selector("exact","member-one","")).toEqual({ok:true,value:{username:"member-one",usernames:null}});
    expect(selector("any","","member, other")).toEqual({ok:true,value:{username:null,usernames:["member","other"]}});
  });
  it("validates untrusted worker command fields",()=>{
    expect(workerSelector(undefined,undefined)).toEqual({username:null,usernames:null});
    expect(workerSelector("exact-name",null)).toEqual({username:"exact-name",usernames:null});
    expect(workerSelector(null,["ab"])).toEqual({username:null,usernames:["ab"]});
    for(const [one,many] of [["a",["bc"]],[1,null],[null,[]],[null,"x"],[null,[1]],[null,Array.from({length:MAX_TERMS+1},()=>"ab")]] as const)
      expect(()=>workerSelector(one,many)).toThrow("vault_invalid_input");
  });
  it("describes the exact scope of a preview",()=>{
    expect(scopeLabel("all","","")).toBe("파일의 모든 항목");
    expect(scopeLabel("exact"," Member ","")).toContain('" Member "');
    expect(scopeLabel("any","","Alpha, beta")).toContain('"Alpha", "beta"');
    expect(formatMiB(26*1024*1024)).toBe("26.0MiB");
  });
  it("small ordinary imports are preselected; an over-capacity file selects nothing instead of silently trimming",()=>{
    const small=Array.from({length:883},(_,n)=>({row:n+2,status:"new"}));
    expect(initialSelection(small,MAX_ITEMS)).toMatchObject({overCapacity:false,available:883});
    expect(initialSelection(small,MAX_ITEMS).rows).toHaveLength(883);
    const rows=Array.from({length:1100},(_,n)=>({row:n+2,status:n%10===9?"duplicate":"new"}));
    const all=initialSelection(rows,MAX_ITEMS);
    expect(all.rows).toHaveLength(990);expect(all.overCapacity).toBe(false);
    expect(initialSelection(rows,117)).toEqual({rows:[],overCapacity:true,available:990});
    expect(initialSelection(rows,0)).toEqual({rows:[],overCapacity:true,available:990});
  });
  it("reports total/capacity before commit",()=>{
    expect(selectionState(883,0,MAX_ITEMS)).toEqual({total:883,limit:MAX_ITEMS,over:false,empty:false});
    expect(selectionState(150,883,117)).toMatchObject({total:1033,over:true});
    expect(selectionState(0,10,990).empty).toBe(true);
  });
});
