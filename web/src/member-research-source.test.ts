import { expect, it } from "vitest";
import { explicitSearchTopic, isMemberPublicSearchUrl, memberPublicSearchStart, startMemberPublicSearch } from "./member-research-source";

it("builds a fixed https search URL from an explicit public topic", () => {
  const start = memberPublicSearchStart("Open Science Fund 2027 신청 자격");
  expect(start.ok).toBe(true);
  if (!start.ok) return;
  const url = new URL(start.url);
  expect(url.origin).toBe("https://duckduckgo.com");
  expect([...url.searchParams.keys()]).toEqual(["q"]);
  expect(url.searchParams.get("q")).toBe("Open Science Fund 2027 신청 자격");
  expect(isMemberPublicSearchUrl(start.url)).toBe(true);
});

it("refuses private-looking, URL-like, empty or oversized topics", () => {
  const reason = (t: string, terms: string[] = []) => { const s = memberPublicSearchStart(t, terms); return s.ok ? "ok" : s.reason; };
  expect(reason("   ")).toBe("empty");
  expect(reason("kim@example.com 주문")).toBe("private_looking");
  expect(reason("my card 4111 1111 1111 1111")).toBe("private_looking");
  expect(reason("010-1234-5678 배송")).toBe("private_looking");
  expect(reason("내 비밀번호 찾기")).toBe("private_looking");
  expect(reason("https://shop.example/offers")).toBe("contains_url");
  expect(reason("a ".repeat(60))).toBe("too_long");
  expect(reason("Kfund reviewer questions", ["Reviewer Questions"])).toBe("contains_private_term");
});

it("only searches when the member explicitly asked for a search", () => {
  expect(explicitSearchTopic("주전자 최저가를 12월 10일까지 도착하는 걸로 비교해줘")).toBeNull();
  expect(explicitSearchTopic("Compare the kettle offers I read and the mail from Kim")).toBeNull();
  expect(explicitSearchTopic('search the web for "Open Science Fund 2027"')).toBe("Open Science Fund 2027");
  expect(explicitSearchTopic("please look up Open Science Fund 2027 eligibility.")).toBe("Open Science Fund 2027 eligibility");
  expect(explicitSearchTopic("오픈사이언스 연구지원 공고 검색해줘")).toBe("오픈사이언스 연구지원 공고");
  expect(startMemberPublicSearch("hello")).toBeNull();
});

it("never accepts a URL this helper did not build", () => {
  expect(isMemberPublicSearchUrl("http://duckduckgo.com/?q=a")).toBe(false);
  expect(isMemberPublicSearchUrl("https://duckduckgo.com/?q=a&kl=kr")).toBe(false);
  expect(isMemberPublicSearchUrl("https://evil.example/?q=a")).toBe(false);
  expect(isMemberPublicSearchUrl("https://user:pw@duckduckgo.com/?q=a")).toBe(false);
  expect(isMemberPublicSearchUrl("not a url")).toBe(false);
});

it("uses only the named public topic in a mixed request and keeps private sources out of search", () => {
  expect(explicitSearchTopic('내 메일 "중간 결과"를 참고하고 “봄 연구 펀드”를 검색해서 지원 초안을 써 줘')).toBe("봄 연구 펀드");
  expect(startMemberPublicSearch('내 메일 제목을 검색해줘')?.ok).toBe(false);
  expect(startMemberPublicSearch('“연구 공고”를 검색하지 마')).toBeNull();
  expect(explicitSearchTopic('“미공개 중간 결과”를 참고하고 지원서 초안을 찾아봐')).toBeNull();
});
