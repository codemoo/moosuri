import { expect, it, vi } from "vitest";
import { googleLogins, isGoogleLogin, googleAccounts } from "./google-login";
import type { Command, Summary } from "./types";

const email = "member@example.test";
const origin = "https://accounts.google.com";
it("offers distinct Google emails in the owner's vault without reading passwords or other origins", () => {
  const entries = [
    {id:"one",kind:"login",email:"first@gmail.com",url:origin},
    {id:"two",kind:"login",email:"second@gmail.com",url:origin},
    {id:"three",kind:"login",email:"third@gmail.com",url:"https://evil.test"},
    {id:"four",kind:"note",email:"fourth@gmail.com",url:origin},
  ];
  const read = vi.fn((q:Command) => q.op === "list" ? entries.map(({id,kind})=>({id,kind}))
    : q.field === "username" ? entries.find(e=>e.id===q.item)!.email
    : q.field === "url" ? entries.find(e=>e.id===q.item)!.url : (()=>{throw Error("secret read")})());
  expect(googleAccounts(read as <T>(q:Command)=>T).map(e=>e.email)).toEqual(["first@gmail.com","second@gmail.com"]);
  expect(read.mock.calls[0][0]).toEqual({op:"list",query:""});
});
const cases = [
  [origin + "/signin", email, true],
  [origin, email.toUpperCase(), true],
  [origin, "other@example.test", false],
  ["https://accounts.google.com.example.test", email, false],
  ["https://member@accounts.google.com", email, false],
  ["http://accounts.google.com", email, false],
  ["https://accounts.google.com:8443", email, false],
] as const;
it.each(cases)("checks the exact origin and account for %s", (address, username, expected) => {
  expect(isGoogleLogin(address, username, email)).toBe(expected);
});
it("looks up only account and address fields and preserves ambiguous choices", () => {
  const read = vi.fn();
  const first = {kind:"login",id:"one"} as Summary, second = {...first, id:"two"};
  read.mockImplementation((q: Command) => q.op === "list" ? [first, second, {kind:"note",id:"three"}]
    : q.field === "username" ? email : origin);
  expect(googleLogins(email, read)).toEqual([first, second]);
  expect(read.mock.calls.map(([q])=>q.field)).toEqual([undefined, "username", "url", "username", "url"]);
});
