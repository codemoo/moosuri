export type Owner={account:string;tenant:string;installation:string;epoch:number};
export type Summary={id:string;revision:number;title:string;kind:"login"|"api_key"|"note";favorite:boolean;expires_at:string;has_totp:boolean};
export type Receipt={operation:string;payload_sha256:string;state:string;header_sha256:string;inventory_sha256:string;commitment:string|null};
export type Operation={kind:"key"|"put"|"delete";id:string}&Record<string,unknown>;
export type State={owner:Owner;header:unknown|null;header_sha256:string;inventory_sha256:string;items:unknown[];pending:Receipt[]};
export type Preview={eligible:number;duplicates:number;filtered:number;rows:{row:number;status:string;blank_password:boolean;unsupported_url:boolean;invalid_otp:boolean}[]};
export type Draft={kind:Summary["kind"];title:string;username:string;url:string;notes:string;favorite:boolean;expires_at:string;secret?:string|null;totp?:string|null};
export type Command={op:string}&Record<string,unknown>;

export type VaultHead={header_sha256:string;revision:number;envelope_epoch:number;generation:number};
export type CurrentSession={owner:Owner;head:VaultHead};
