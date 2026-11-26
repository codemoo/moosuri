const encoder = new TextEncoder();

export function newPasswordIssue(value: string): string | null {
  if (Array.from(value).length < 8) return "8자 이상 입력해 주세요.";
  if (encoder.encode(value).length > 1024) return "UTF-8 기준 1,024바이트 이내로 입력해 주세요.";
  return null;
}
