// Both watch types use fixed-second cadences and a lifetime in whole days.
export type WatchEvery = 1800 | 3600 | 86400;
export type WatchTiming =
  | { ok: true; every_seconds: WatchEvery | null; days: number | null; cadence: boolean }
  | { ok: false; reason: "unsupported_interval" | "unsupported_duration"; conflict: boolean; cadence: boolean };

const DAY = 86400;
const units: Record<string, number> = {
  초: 1, 분: 60, 시간: 3600, 일: DAY, 주: 7 * DAY, 주일: 7 * DAY,
  달: 30 * DAY, 개월: 30 * DAY, 년: 365 * DAY,
};
const counts: Record<string, number> = {
  반: 0.5, 한: 1, 두: 2, 세: 3, 네: 4, 다섯: 5, 여섯: 6, 일곱: 7, 여덟: 8, 아홉: 9,
};
const tens: Record<string, number> = { 열: 10, 스무: 20, 스물: 20, 서른: 30, 마흔: 40, 쉰: 50, 예순: 60, 일흔: 70, 여든: 80, 아흔: 90 };
const namedDays: Record<string, number> = {
  하루: 1, 이틀: 2, 사흘: 3, 나흘: 4, 닷새: 5, 엿새: 6, 일주일: 7, 열흘: 10, 보름: 15,
};
const repeats: Record<string, number> = {
  매초: 1, 매분: 60, 매시간: 3600, 매일: DAY, 격일: 2 * DAY, 매주: 7 * DAY,
  격주: 14 * DAY, 매월: 30 * DAY, 매달: 30 * DAY, 초당: 1,
};

function alternatives(words: Record<string, number>) {
  return Object.keys(words).sort((a, b) => b.length - a.length).join("|");
}

// Recognize a whole quantity before interpreting its final marker. For example,
// the unit of "1시간 30분마다" is a 90-minute interval and "48시간 동안" is a two-day lifetime.
const native = "(?:" + alternatives(tens) + ")(?:\\s*(?:" + alternatives(counts) + "))?";
const count = "[+-]?\\d+(?:\\.\\d+)?|[일이삼사오육칠팔구십백천]+|" + native + "|" + alternatives(counts);
const amount = "(?:(?<named>" + alternatives(namedDays) + ")|(?<number>" + count + ")\\s*(?<unit>" + alternatives(units) + "))";
const cadenceEnd = "마다|간격(?:으로)?|에\\s*(?:한|1)\\s*번";
const marker = new RegExp("^\\s*(?:(?<cadence>" + cadenceEnd + ")|(?<duration>동안|간|까지))", "u");
const start = [
  // A calendar deadline requires a civil date. The storage contract accepts a lifetime in days.
  "(?<deadline>(?:\\d{4}[-./]\\d{1,2}[-./]\\d{1,2}|(?:\\d{1,2}월\\s*)?\\d{1,2}일|오늘|내일|모레|[월화수목금토일]요일)\\s*까지)",
  "(?<perDay>하루|매\\s*일|1\\s*일)\\s*(?:에|당)?\\s*(?<times>" + count + ")\\s*(?:번|회)",
  "(?<each>매\\s*)?" + amount,
  "(?<repeat>매\\s*(?:시간|일|주|월|달|분|초)|격주|격일|초당)(?:\\s*마다)?",
  "(?<bare>시간|분|초)\\s*(?:" + cadenceEnd + ")",
].join("|");

function countValue(word: string): number {
  word = word.replace(/\s+/g, "");
  if (word in counts) return counts[word];
  for (const [prefix, value] of Object.entries(tens)) {
    if (word.startsWith(prefix)) return value + (counts[word.slice(prefix.length)] ?? 0);
  }
  if (!/^[일이삼사오육칠팔구십백천]+$/.test(word)) return Number(word);
  let total = 0, digit = 0;
  for (const char of word) {
    const power = "십백천".indexOf(char);
    if (power < 0) digit = "일이삼사오육칠팔구".indexOf(char) + 1;
    else { total += (digit || 1) * 10 ** (power + 1); digit = 0; }
  }
  return total + digit;
}

function quantityInSeconds(groups: Record<string, string>) {
  if (groups.named) return namedDays[groups.named] * DAY;
  const value = countValue(groups.number);
  return value >= 0 ? value * units[groups.unit] : NaN;
}

function readTiming(text: string) {
  const intervals = new Set<number>(), durations = new Set<number>();
  const first = new RegExp(start, "gu"), next = new RegExp("\\s*" + amount, "yu");
  let kept = "", last = 0;
  for (let token; (token = first.exec(text));) {
    const head = token.groups!;
    let end = first.lastIndex;
    if (head.deadline) durations.add(NaN);
    else if (head.perDay) intervals.add(countValue(head.times) === 1 ? DAY : NaN);
    else if (head.repeat) intervals.add(repeats[head.repeat.replace(/\s+/g, "")]);
    else if (head.bare) intervals.add(units[head.bare]);
    else {
      let seconds = quantityInSeconds(head);
      let unit = head.named ? DAY : units[head.unit];
      next.lastIndex = end;
      for (let more; (more = next.exec(text));) {
        seconds += quantityInSeconds(more.groups!);
        unit = more.groups!.named ? DAY : units[more.groups!.unit];
        end = next.lastIndex;
      }
      const half = /^\s*반/.exec(text.slice(end));
      if (half) { seconds += unit / 2; end += half[0].length; }
      const suffix = marker.exec(text.slice(end));
      if (head.each || suffix?.groups?.cadence) intervals.add(seconds);
      else if (suffix?.groups?.duration) durations.add(seconds / DAY);
      // Standalone "일주일" and "한 달" are also accepted as lifetimes.
      else if (/^(일주일|한\s*달)$/.test(token[0]) && !/^[가-힣]/.test(text.slice(end))) durations.add(seconds / DAY);
      else continue;
      end += suffix?.[0].length ?? 0;
    }
    kept += text.slice(last, token.index) + " ";
    last = first.lastIndex = end;
  }
  return { intervals, durations, remaining: kept + text.slice(last) };
}

export function parseWatchTiming(text: string): WatchTiming {
  const { intervals, durations } = readTiming(text);
  const cadence = intervals.size > 0;
  const [every = null] = intervals;
  const [days = null] = durations;
  if (intervals.size > 1 || (every !== null && every !== 1800 && every !== 3600 && every !== DAY))
    return { ok: false, reason: "unsupported_interval", conflict: intervals.size > 1, cadence };
  if (durations.size > 1 || (days !== null && (!Number.isInteger(days) || days < 1 || days > 90)))
    return { ok: false, reason: "unsupported_duration", conflict: durations.size > 1, cadence };
  return { ok: true, every_seconds: every, days, cadence };
}

export function stripWatchTiming(text: string): string {
  return readTiming(text).remaining;
}
