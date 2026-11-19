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

export function displayAddress(address: string) {
  try {
    const hasScheme = /^[a-z][a-z\d+.-]*:\/\//i.test(address);
    const url = new URL(hasScheme ? address : `https://${address}`);
    if (url.protocol === "http:" || url.protocol === "https:") return url.host;
  } catch {
    // Addresses from older imports can use formats that URL does not parse.
  }
  return "저장된 주소";
}

export function VaultIcon({name}: {name: "back" | "next" | "add" | "edit" | "search" | "menu"}) {
  return <span className={`vault-icon icon-${name}`} aria-hidden="true"/>;
}

export function VaultList({items, query, disabled, onOpen}: {
  items: Summary[];
  query: string;
  disabled: boolean;
  onOpen: (item: Summary) => void;
}) {
  if (!items.length) {
    return <p className="vault-card vault-row">{query ? "맞는 항목이 없습니다." : "첫 항목을 추가해 보세요."}</p>;
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
    <div className="vault-value"><span className="vault-sr">{name} </span><span>{children}</span></div>
    {onReveal && <button type="button" className="vault-field-action" aria-label={`${name} 보기`} disabled={disabled} onClick={onReveal}>보기</button>}
    {onCopy && <button type="button" className="vault-field-action" aria-label={`${name} 복사`} disabled={disabled} onClick={onCopy}>복사</button>}
  </div>;
}
