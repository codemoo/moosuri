export const ACCOUNT_PASSWORD_HELP =
  "Use at least 8 characters, up to 256 UTF-8 bytes.";

export function accountPasswordValid(password: string): boolean {
  return Array.from(password).length >= 8 &&
    new TextEncoder().encode(password).length <= 256;
}
