import type {ReactNode} from "react";
import type {Summary} from "./types";

const groups = [
  {kind: "login", label: "로그인"},
  {kind: "api_key", label: "API 키"},
  {kind: "note", label: "보안 메모"},
] as const;

export function itemName(item: Summary) {
  return item.title || "이름 없는 항목";
}

const iconPaths = {
  back: "m15 5-7 7 7 7",
  next: "m9 5 7 7-7 7",
  add: "M12 5v14M5 12h14",
  edit: "m14 6 4 4m-14 10 4-1 12-12a2.8 2.8 0 0 0-4-4L4 15z",
  search: "M11 4a7 7 0 1 0 0 14 7 7 0 0 0 0-14zm5 12 4 4",
  settings: "M4 7h9m4 0h3M4 17h3m4 0h9M13 4v6M8 14v6",
  lock: "M5 10h14v11H5zm3 0V7a4 4 0 0 1 8 0v3m-4 5v2",
  login: "M15 10a5 5 0 1 0-4 5l2-2h3v-3zm-8-2h.01",
  api_key: "m8 7-5 5 5 5m8-10 5 5-5 5m-3-13-2 16",
  note: "M5 3h10l4 4v14H5zM8 11h8m-8 4h6",
} as const;

export function VaultIcon({name}: {name: keyof typeof iconPaths}) {
  return <svg aria-hidden="true" focusable="false" className="vault-icon" viewBox="0 0 24 24"
    width="20" height="20" fill="none" stroke="currentColor" strokeWidth="1.65"
    strokeLinecap="round" strokeLinejoin="round"><path d={iconPaths[name]}/></svg>;
}

export function VaultList({items, query, disabled, onOpen}: {
  items: Summary[];
  query: string;
  disabled: boolean;
  onOpen: (item: Summary) => void;
}) {
  if (!items.length) {
    return <div className="vault-empty member-page-list">
      <VaultIcon name={query ? "search" : "lock"}/>
      <p>{query ? "맞는 항목이 없습니다." : "아직 보관한 항목이 없습니다."}</p>
      <small>{query ? "검색어 또는 저장한 계정을 확인해 주세요." : "새 항목에서 로그인, API 키, 보안 메모를 추가하세요."}</small>
    </div>;
  }

  return <div className="vault-groups">
    {groups.map(({kind, label}) => {
      const entries = items.filter(item => item.kind === kind)
        .sort((a, b) => itemName(a).localeCompare(itemName(b), "ko", {numeric: true}));
      if (!entries.length) return null;
      return <section className="vault-group" key={kind} aria-labelledby={`group-${kind}`}>
        <h2 id={`group-${kind}`} className="vault-caption">{label}</h2>
        <ul className="vault-card vault-rows">
          {entries.map(item => <li key={item.id} className="vault-row">
            <button type="button" className="vault-entry" disabled={disabled} onClick={() => onOpen(item)}>
              <span className="vault-item-icon"><VaultIcon name={kind}/></span>
              <span className="vault-value">{itemName(item)}</span>
              {item.favorite && <span className="vault-favorite" aria-label="즐겨찾기">★</span>}
              <VaultIcon name="next"/>
            </button>
          </li>)}
        </ul>
      </section>;
    })}
  </div>;
}

export function VaultField({name, children, onReveal, onCopy, disabled}: {
  name: string;
  children: ReactNode;
  onReveal?: () => void;
  onCopy?: () => void;
  disabled: boolean;
}) {
  return <div className="vault-row vault-field">
    <div className="vault-value"><span className="vault-field-label">{name}</span><span>{children}</span></div>
    {onReveal && <button type="button" className="vault-field-action" aria-label={`${name} 보기`} disabled={disabled} onClick={onReveal}>보기</button>}
    {onCopy && <button type="button" className="vault-field-action" aria-label={`${name} 복사`} disabled={disabled} onClick={onCopy}>복사</button>}
  </div>;
}
