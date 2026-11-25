/** A price watch follows one displayed KRW price on one public page. */
import { stripWatchTiming } from "./member-watch-timing";

export type PriceCondition = { won: number; inclusive: boolean };

const MAX_WON = 999_999_999_999;
const WON = /([+\-−]?\d[\d.,]*(?:\s*[억만천백십]+\s*\d[\d.,]*)*\s*[억만천백십]*)\s*원\s*(이하|이내|미만|아래|밑|이상|초과|보다\s*(?:낮|싸|저렴|비싸|높))/gu;
const PRICE_BOUND = /(?:원|달러|유로|엔|%)\s*(?:이하|이내|미만|아래|밑|이상|초과|보다|넘|떨어|내려)|(?:가격|요금|숙박비|판매가|금액).{0,35}(?:내려가|떨어지|저렴해|한도|상한|예산|미만|이하|만\s*(?:바뀌|변하))/u;
const POWERS: Record<string, number> = { 억: 8, 천만: 7, 백만: 6, 십만: 5, 만: 4, 천: 3, 백: 2, 십: 1 };
// Timing has already been consumed. Anything beyond the one comparison and a notification
// request can change its meaning (a second requirement, a negation, or another action).
const NOTIFY_WORDS = /(?:그\s*때(?:만)?\s*)?(?:(?:나한테|저한테|나에게|저에게)\s*)?(?:알려|말해|통지해|알림(?:을|만)?\s*(?:보내)?)(?:\s*(?:줄래(?:요)?|주세요|줘|주라))?/u;
const PRICE_SUFFIX = /^(?:이|가|로|으로|라|인|일|될|되는|되었을|됐을|되|내려가|내려갈|내려갔을|떨어지|떨어질|떨어졌을|낮아지|낮아질|아지|아질|지|질|하|으|면|때|경우|에|만)*$/u;

const EXTRA_FILTER = /[%$€¥₩~∼～]|(?:\d+|한|두|세|네)\s*(?:인|명)|세금|조식|취소|주말|평일|요일|재고|빈\s*방|포함|제외|또는|혹은|거나|면서|라면|으면|일\s*때|할\s*때|예약해|결제해|구매해|주문해/u;

function wonValue(input: string): number | null {
  // Each Korean scale must decrease. Never read the tail of an unsupported or malformed amount.
  const amount = input.replace(/\s+/gu, "");
  const part = /([1-9]\d{0,2}(?:,\d{3})+|\d+)(?:\.(\d+))?(천만|백만|십만|억|만|천|백|십)?/gy;
  let end = 0, total = 0, previous = Infinity;
  while (end < amount.length) {
    const match = part.exec(amount);
    if (!match) return null;
    const power = POWERS[match[3]] ?? 0;
    const fraction = match[2] ?? "";
    if (fraction.length > power) return null;
    const scale = 10 ** power;
    const value = Number(match[1].replace(/,/g, "") + fraction.padEnd(power, "0"));
    if (scale >= previous || !Number.isSafeInteger(value) || (end > 0 && value >= previous)) return null;
    total += value;
    previous = scale;
    end = part.lastIndex;
  }
  return total > 0 && total <= MAX_WON ? total : null;
}

export function parsePriceCondition(text: string): PriceCondition | "unsupported" | null {
  text = stripWatchTiming(text);
  const limits = [...text.matchAll(WON)];
  if (!limits.length) return PRICE_BOUND.test(text) ? "unsupported" : null;
  if (limits.length !== 1) return "unsupported";
  const limit = limits[0], [, amount, bound] = limit;
  const before = text.slice(0, limit.index);
  const after = text.slice(limit.index + limit[0].length).replace(NOTIFY_WORDS, "").replace(/[\s,.!?？]+/gu, "");
  if (EXTRA_FILTER.test(before) || !PRICE_SUFFIX.test(after)) return "unsupported";
  const won = wonValue(amount);
  if (won === null || /^(?:이상|초과)|비싸|높/u.test(bound)) return "unsupported";
  return { won, inclusive: bound === "이하" || bound === "이내" };
}

export function samePriceCondition(a?: PriceCondition, b?: PriceCondition): boolean {
  return a?.won === b?.won && a?.inclusive === b?.inclusive;
}

export function validPriceCondition(value: unknown): value is PriceCondition {
  if (!value || typeof value !== "object" || !("won" in value) || !("inclusive" in value)) return false;
  return typeof value.won === "number" && Number.isSafeInteger(value.won) && value.won > 0 &&
    value.won <= MAX_WON && typeof value.inclusive === "boolean";
}

export const priceLabel = (condition: PriceCondition) =>
  condition.won.toLocaleString("ko-KR") + "원 " + (condition.inclusive ? "이하" : "미만");
