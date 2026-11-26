import { describe, expect, it } from "vitest";
import { newPasswordIssue } from "./password";

describe("new vault passwords", () => {
  it.each(["1234567", "가나다라마바사", "🔑".repeat(7)])("rejects seven characters", value => {
    expect(newPasswordIssue(value)).toBe("8자 이상 입력해 주세요.");
  });
  it.each(["12345678", "가나다라마바사아", "🔑".repeat(8), "a longer passphrase", "🔑".repeat(256)])(
    "accepts eight or more characters within the UTF-8 limit", value => {
      expect(newPasswordIssue(value)).toBeNull();
    },
  );
  it("enforces the byte limit even for multibyte characters", () => {
    expect(newPasswordIssue("🔑".repeat(256) + "a")).toContain("1,024바이트");
  });
});
