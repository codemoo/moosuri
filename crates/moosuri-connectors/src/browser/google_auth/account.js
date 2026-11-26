function(operation, email, secret) {
  const visible = e => !!e && !!e.getClientRects().length && getComputedStyle(e).visibility !== "hidden";
  const accounts = location.origin === "https://accounts.google.com";
  const home = location.origin === "https://myaccount.google.com";
  if (location.href === "about:blank" || document.readyState === "loading") return "loading";
  if (!accounts && !home) return "rejected";
  const text = (document.body?.innerText || "").toLowerCase();
  if (["browser or app may not be secure", "couldn’t sign you in", "couldn't sign you in",
       "브라우저 또는 앱이 안전하지", "로그인할 수 없음"].some(s => text.includes(s))) return "rejected";
  const matches = value => [...value.matchAll(/[a-z0-9.!#$%&'*+\/=?^_`{|}~-]+@[a-z0-9.-]+/gi)]
    .map(m => m[0].trim().toLowerCase());
  const labels = [...document.querySelectorAll("[data-email], #profileIdentifier, [aria-label]")]
    .filter(visible).flatMap(e => matches(e.getAttribute("data-email") || e.getAttribute("aria-label") || e.textContent || ""));
  const emails = [...new Set(labels)];
  const exact = emails.length === 1 && emails[0] === email.toLowerCase();
  const different = emails.length > 0 && !exact;
  if (home) return exact ? "ready" : different ? "mismatch" : "unknown";
  const input = [...document.querySelectorAll("input[type=email]")].filter(e => visible(e) && !e.disabled && !e.readOnly);
  const passwords = [...document.querySelectorAll("input[type=password]")].filter(e => visible(e) && !e.disabled && !e.readOnly);
  const submit = (field, next, value) => {
    if (!visible(next) || next.disabled || !field || typeof value !== "string") return "unknown";
    const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set;
    if (!setter) return "unknown";
    setter.call(field, value);
    field.dispatchEvent(new Event("input", {bubbles:true}));
    field.dispatchEvent(new Event("change", {bubbles:true}));
    next.click();
    return "submitted";
  };
  if (input.length === 1 && !passwords.length) {
    if (operation === "identifier") return submit(input[0], document.getElementById("identifierNext"), email);
    return "identifier";
  }
  if (different) return "mismatch";
  if (passwords.length === 1 && exact) {
    if (operation === "password") return submit(passwords[0], document.getElementById("passwordNext"), secret);
    return "password";
  }
  if (["check your phone", "verify it’s you", "verify it's you", "2-step verification",
       "휴대전화 확인", "본인 인증", "2단계 인증"].some(s => text.includes(s))) return "verification";
  return "unknown";
}
