// One live vault selection. Only a Chrome toolbar click can choose a destination.
// Suspension, vault lock, navigation or expiry drops the request.
const site = chrome.runtime.getManifest().externally_connectable.matches[0].slice(0, -2);
const accounts = "https://accounts.google.com";
let pending;

function finish(job, status) {
  if (pending !== job) return;
  pending = undefined;
  clearTimeout(job.timer);
  try { job.port.postMessage({ status }); } catch {}
  job.port.disconnect();
}

// Chrome serializes this function into the isolated world of the selected document.
// Check everything again at fill time. Never return page content or submit the form.
function googlePassword(email, secret) {
  if (window !== window.top || document.visibilityState !== "visible" ||
      location.origin !== "https://accounts.google.com" ||
      !/^\/(?:v3\/signin|signin\/v2)\/challenge\/pwd\/?$/u.test(location.pathname)) return false;
  const visible = element => element.getClientRects().length > 0 && getComputedStyle(element).visibility === "visible";
  const identities = Array.from(document.querySelectorAll("#displayIdentifier, #profileIdentifier, [data-email]"))
    .filter(visible).map(el => (el.getAttribute("data-email") || el.textContent || "").trim().toLowerCase())
    .filter(value => /^[^@\s]+@[^@\s]+$/u.test(value));
  if (!identities.length || identities.some(value => value !== email.toLowerCase())) return false;
  const fields = Array.from(document.querySelectorAll('input[type="password"]')).filter(visible);
  if (fields.length !== 1) return false;
  const field = fields[0];
  if (field.disabled || field.readOnly || field.value || field.name !== "Passwd" ||
      field.autocomplete === "new-password" ||
      (field.form && new URL(field.form.action || location.href).origin !== location.origin)) return false;
  if (secret !== undefined) {
    field.value = secret;
    field.dispatchEvent(new Event("input", { bubbles: true }));
    field.dispatchEvent(new Event("change", { bubbles: true }));
  }
  return true;
}

chrome.runtime.onConnectExternal.addListener(port => {
  if (port.name !== "google-login" || port.sender?.frameId !== 0 ||
      port.sender.origin !== site || !port.sender.tab || new URL(port.sender.url).origin !== site) {
    port.disconnect();
    return;
  }
  if (pending) {
    port.postMessage({ status: "busy" });
    port.disconnect();
    return;
  }
  const job = { port, state: "opening", vaultTab: port.sender.tab.id, expires: Date.now() + 120_000 };
  pending = job;
  job.timer = setTimeout(() => finish(job, "expired"), 120_000);
  port.onDisconnect.addListener(() => {
    if (pending === job) { clearTimeout(job.timer); pending = undefined; }
  });
  port.onMessage.addListener(async message => {
    if (pending !== job || Date.now() >= job.expires) return finish(job, "expired");
    if (message?.kind === "ping" && job.state !== "opening") return;
    if (job.state === "opening" && message?.kind === "arm" && typeof message.email === "string" &&
        message.email.length <= 254 && /^[^@\s]+@[^@\s]+$/u.test(message.email)) {
      job.email = message.email;
      job.state = "armed";
      port.postMessage({ status: "armed" });
      return;
    }
    if (job.state !== "requested" || message?.kind !== "credential" ||
        typeof message.value !== "string" || !message.value || message.value.length > 4096) {
      return finish(job, "stopped");
    }
    job.state = "filling";
    try {
      const [result] = await chrome.scripting.executeScript({
        target: { tabId: job.tab, documentIds: [job.document] }, func: googlePassword, args: [job.email, message.value],
      });
      finish(job, result?.frameId === 0 && result.documentId === job.document && result.result === true ? "filled" : "stopped");
    } catch { finish(job, "stopped"); }
  });
});

chrome.action.onClicked.addListener(async tab => {
  const job = pending;
  if (!job || job.state !== "armed") return;
  if (Date.now() >= job.expires) return finish(job, "expired");
  if (!tab.active || !Number.isInteger(tab.id) || tab.id === job.vaultTab || !tab.url?.startsWith(accounts + "/")) {
    return finish(job, "stopped");
  }
  job.state = "checking";
  try {
    const [result] = await chrome.scripting.executeScript({
      target: { tabId: tab.id, frameIds: [0] }, func: googlePassword, args: [job.email],
    });
    if (pending !== job || Date.now() >= job.expires || result?.frameId !== 0 ||
        !result.documentId || result.result !== true) return finish(job, "stopped");
    job.state = "requested";
    job.tab = tab.id;
    job.document = result.documentId;
    job.port.postMessage({ status: "request" });
  } catch { finish(job, "stopped"); }
});
