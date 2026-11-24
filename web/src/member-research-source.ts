/* Start of a public web search for the member's research outcomes when no source page is known yet.
 *
 * This only BUILDS a URL for the existing member browser (`onOpenBrowser(url)`); it never fetches,
 * never opens anything itself and never reads a page. The URL is a fixed HTTPS origin plus one `q`
 * parameter that holds an explicitly requested public topic, bounded in size. Mail, names, addresses,
 * numbers that look like identifiers and anything else from the conversation never go into it. */

export const PUBLIC_SEARCH_ORIGIN = "https://duckduckgo.com";
export const PUBLIC_SEARCH_MAX_CHARS = 80;
export const PUBLIC_SEARCH_MAX_WORDS = 10;

export type PublicSearchRefusal =
  | "empty" | "too_long" | "private_looking" | "contains_url" | "contains_private_term";
export type PublicSearchStart =
  | { ok: true; topic: string; url: string }
  | { ok: false; reason: PublicSearchRefusal };

const PRIVATE_SHAPES: RegExp[] = [
  /[^\s@]+@[^\s@]+/,                              // e-mail address
  /\d[\d\s().-]{6,}\d/,                            // phone / card / account / registration numbers
  /\b\d{6}\s*-\s*\d{7}\b/,                         // Korean resident registration number
  /(password|passcode|secret|token|api[-_ ]?key|otp)/i,
  /(?:메일|편지함|받은편지|주소록|내부|비공개|사내|담당자|담당 교수|이름|연락처|우리 집|집주소|내 (?:연구|자료|문서|기억)|\b(?:my|inbox|e-?mail|contact|internal|confidential|private)\b)/i,
  /(비밀번호|암호|인증\s?번호|주민\s?등록|계좌|카드\s?번호|토큰)/,
];
const URL_SHAPE = /(https?:\/\/|www\.|\b[a-z0-9-]+\.(com|net|org|io|kr|co|dev|app)\b)/i;
// Control characters and invisible/bidi formatting characters, written as escapes in a string.
const CONTROL = new RegExp("[\\u0000-\\u001f\\u007f-\\u009f\\u200b-\\u200f\\u202a-\\u202e\\u2066-\\u2069]", "g");

function normalise(topic: string): string {
  return topic.replace(CONTROL, " ").replace(/\s+/g, " ").trim();
}

/** Build the public search URL for `topic`, or say why not. `privateTerms` are strings the caller
 * knows are private (member name, e-mail, a mail subject, an account label): a topic containing any
 * of them is refused instead of being sent. */
export function memberPublicSearchStart(topic: string, privateTerms: readonly string[] = []): PublicSearchStart {
  const text = normalise(topic);
  if (!text) return { ok: false, reason: "empty" };
  if (text.length > PUBLIC_SEARCH_MAX_CHARS || text.split(" ").length > PUBLIC_SEARCH_MAX_WORDS) return { ok: false, reason: "too_long" };
  if (PRIVATE_SHAPES.some(shape => shape.test(text))) return { ok: false, reason: "private_looking" };
  if (URL_SHAPE.test(text)) return { ok: false, reason: "contains_url" };
  const lower = text.toLowerCase();
  if (privateTerms.some(term => { const t = normalise(term).toLowerCase(); return t.length >= 2 && lower.includes(t); })) {
    return { ok: false, reason: "contains_private_term" };
  }
  const url = new URL(PUBLIC_SEARCH_ORIGIN + "/");
  url.searchParams.set("q", text);
  return { ok: true, topic: text, url: url.toString() };
}

/** True only for a URL this helper could have produced: HTTPS, the fixed origin, exactly one `q`. */
export function isMemberPublicSearchUrl(value: string): boolean {
  try {
    const url = new URL(value);
    const keys = [...url.searchParams.keys()];
    return url.protocol === "https:" && url.origin === PUBLIC_SEARCH_ORIGIN && url.username === "" && url.password === ""
      && url.pathname === "/" && url.hash === "" && keys.length === 1 && keys[0] === "q" && normalise(url.searchParams.get("q") ?? "") !== "";
  } catch {
    return false;
  }
}

// Extract only a topic grammatically attached to the search verb. Other clauses may name private sources.
const QUOTED_KO = /["“‘「『']([^"“”‘’「」『』']{2,80})["”’」』'](?:을|를)?\s*(?:(?:공개\s*)?(?:웹|인터넷|온라인)(?:에서)?\s*)?(?:검색해|검색 해|찾아봐|찾아 봐|알아봐)/u;
const QUOTED_EN = /\b(?:search(?: the web| online)? for|look up(?: online)?|google)\s+["“‘「『']([^"“”‘’「」『』']{2,80})["”’」』']/i;
const ENGLISH_CUE = /^(?:please\s+)?(?:search(?:\s+the\s+web|\s+online)?\s+for|look\s+up(?:\s+online)?|google)\s+([\p{L}\p{N} -]{2,80})[.!?]?$/iu;
const KOREAN_CUE = /^([\p{L}\p{N} -]{2,80}?)\s*(?:을|를)?\s*(?:(?:공개\s*)?(?:웹|온라인|인터넷)(?:에서)?\s*)?(?:검색해|검색 해|찾아봐|찾아 봐|알아봐|알아 봐)(?:줘| 줘| 주세요|주세요)?[.!?]?$/u;

export function explicitSearchTopic(message: string): string | null {
  const text = normalise(message);
  if (!text || /(?:하지|찾지|말하지|보내지|알리지)\s*(?:마|말)|말고|하지는?\s*않|do not|don't|never/i.test(text)) return null;
  const match = text.match(QUOTED_KO) ?? text.match(QUOTED_EN) ?? text.match(ENGLISH_CUE) ?? text.match(KOREAN_CUE);
  return match ? normalise(match[1]) : null;
}

export function startMemberPublicSearch(message: string, privateTerms: readonly string[] = []): PublicSearchStart | null {
  const topic = explicitSearchTopic(message);
  return topic === null ? null : memberPublicSearchStart(topic, privateTerms);
}
