// @vitest-environment node
import {beforeAll, describe, expect, it, vi} from "vitest";
import {webcrypto, randomUUID} from "node:crypto";
import {sealGoogleLogin, type KiriRecipient} from "./kiri-google";
import type {Command, Summary} from "./types";

const account=randomUUID(), tenant=randomUUID(), challenge=randomUUID();
const email="member@example.test", selected="selected@example.test", origin="https://assistant.example.test";
let keys:CryptoKeyPair;
let recipient:Extract<KiriRecipient,{protocol:string}>;
const item={id:randomUUID(),revision:1,email:selected} as Summary & {email:string};
const secret="synthetic-only-password";
const read=vi.fn(async (_command:Command)=>secret);
const run=async <T,>(command:Command)=>await read(command) as T;
const decode=(value:string)=>new Uint8Array(Buffer.from(value,"base64"));
beforeAll(async ()=>{
  vi.stubGlobal("crypto",webcrypto);
  keys=await crypto.subtle.generateKey({name:"RSA-OAEP",modulusLength:3072,publicExponent:new Uint8Array([1,0,1]),hash:"SHA-256"},true,["encrypt","decrypt"]);
  recipient={protocol:"moosuri-google-runtime-v1",spki:Buffer.from(await crypto.subtle.exportKey("spki",keys.publicKey)).toString("base64"),
    binding:{installation:randomUUID(),account,tenant,volume:randomUUID(),generation:3,epoch:0,challenge,email,origin}};
});
describe("Kiri recipient ownership and encryption",()=>{
  it("opens with the Rust protocol field order and the selected Google identity",async ()=>{
    const envelope=await sealGoogleLogin(recipient,account,email,challenge,item,run,{tenant,origin});
    const b=recipient.binding;
    const label=new TextEncoder().encode(["moosuri-google-v3",b.installation,account,tenant,b.volume,3,0,challenge,email,selected,origin].join("\n"));
    const raw=await crypto.subtle.decrypt({name:"RSA-OAEP",label},keys.privateKey,decode(envelope.key));
    const key=await crypto.subtle.importKey("raw",raw,"AES-GCM",false,["decrypt"]);
    expect(new TextDecoder().decode(await crypto.subtle.decrypt({name:"AES-GCM",iv:decode(envelope.iv),additionalData:label},key,decode(envelope.body)))).toBe(secret);
    const other=new TextEncoder().encode(new TextDecoder().decode(label).replace(selected,"other@example.test"));
    await expect(crypto.subtle.decrypt({name:"AES-GCM",iv:decode(envelope.iv),additionalData:other},key,decode(envelope.body))).rejects.toThrow();
  });
  it.each(["account","tenant","challenge","email","origin","epoch"])("rejects a changed %s before reading the vault password",async field=>{
    read.mockClear();
    const binding={...recipient.binding,[field]:field==="epoch"?-1:field==="email"?selected:field==="origin"?"https://other.example.test":randomUUID()};
    await expect(sealGoogleLogin({...recipient,binding},account,email,challenge,item,run,{tenant,origin})).rejects.toThrow("vault_unauthorized");
    expect(read).not.toHaveBeenCalled();
  });
  it("keeps the explicitly configured legacy recipient usable during migration",async ()=>{
    const envelope=await sealGoogleLogin({account,email,spki:recipient.spki},account,email,challenge,item,run);
    const label=new TextEncoder().encode(["moosuri-google-v2",challenge,account,email,selected].join("\n"));
    expect((await crypto.subtle.decrypt({name:"RSA-OAEP",label},keys.privateKey,decode(envelope.key))).byteLength).toBe(32);
  });
});
