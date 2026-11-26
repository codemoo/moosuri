function(email, callback, state) {
  if (location.href === "about:blank" || document.readyState === "loading") return "waiting";
  const expected = new URL(callback);
  if (location.origin === expected.origin) {
    const query = new URL(location.href).searchParams;
    return location.pathname === expected.pathname && !query.has("error") && query.getAll("code").length === 1 &&
      !!query.get("code") && query.getAll("state").length === 1 && query.get("state") === state ? "returned" : "blocked";
  }
  if (location.origin !== "https://accounts.google.com") return "blocked";
  const text = (document.body?.innerText || "").toLowerCase();
  if (["access blocked", "browser or app may not be secure", "액세스 차단"].some(s => text.includes(s))) return "blocked";
  if (["verify it's you", "verify it’s you", "check your phone", "2-step verification", "본인 인증", "2단계 인증"].some(s => text.includes(s))) return "needs_human";
  const visible = e => !!e.getClientRects().length && getComputedStyle(e).visibility !== "hidden";
  const emailMatches = value => [...value.matchAll(/[a-z0-9.!#$%&'*+\/=?^_`{|}~-]+@[a-z0-9.-]+/gi)].map(m => m[0].toLowerCase());
  const controls = [...document.querySelectorAll("button,[role=button],a")].filter(visible);
  const label = e => (e.getAttribute("aria-label") || e.innerText || e.textContent || "").trim().toLowerCase();
  const accounts = controls.filter(e => [e.getAttribute("data-identifier"), e.getAttribute("data-email"), label(e)]
    .some(value => typeof value === "string" && value.toLowerCase() === email.toLowerCase()));
  const labels = [...document.querySelectorAll("[data-email],#profileIdentifier,[aria-label]")].filter(visible)
    .flatMap(e => emailMatches(e.getAttribute("data-email") || e.getAttribute("aria-label") || e.textContent || ""));
  const exact = new Set(labels).size === 1 && labels.includes(email.toLowerCase());
  const buttons = controls.filter(e => ["continue", "allow", "계속", "허용"].includes(label(e)) && !e.disabled);
  const target = exact && buttons.length === 1 ? buttons[0] : accounts.length === 1 ? accounts[0] : null;
  if (!target) return "waiting";
  // Never replay an uncertain click on the same element/document.
  const clicks = window.__moosuriGoogleConsent || (window.__moosuriGoogleConsent = {clicked:new WeakSet(), count:0});
  if (clicks.clicked.has(target)) return "waiting";
  if (clicks.count >= 6) return "needs_human";
  clicks.clicked.add(target); clicks.count++;
  target.click();
  return "waiting";
}
