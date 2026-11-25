"""Linux Chromium integration scenarios. All HTTP pages and credentials are synthetic."""
import base64
import hashlib
import json
import subprocess
import tempfile
from pathlib import Path
from zipfile import ZipFile

from playwright.sync_api import expect, sync_playwright
from package import package

VAULT = "https://vault.example.test"
GOOGLE = "https://accounts.google.com/v3/signin/challenge/pwd"
EMAIL = "member@example.test"
VALUE = "synthetic-password-for-autofill-test"


def main():
    with tempfile.TemporaryDirectory() as folder, sync_playwright() as pw:
        root = Path(folder)
        archive = root / "extension.zip"
        package(VAULT, archive)
        ext = root / "extension"
        with ZipFile(archive) as zipped:
            zipped.extractall(ext)
        manifest = json.loads((ext / "manifest.json").read_text())
        digest = hashlib.sha256(base64.b64decode(manifest["key"])).hexdigest()[:32]
        extension_id = "".join(chr(ord("a") + int(c, 16)) for c in digest)
        assert extension_id == "lcpppplcklhmbmmfionndhkklkaogpma"
        browser = pw.chromium.launch_persistent_context(
            root / "profile", executable_path=pw.chromium.executable_path, headless=False,
            args=["--disable-background-networking", "--host-resolver-rules=MAP * ~NOTFOUND",
                  f"--disable-extensions-except={ext}", f"--load-extension={ext}"],
        )
        vault = browser.pages[0]
        google = browser.new_page()
        def fixture(route):
            url = route.request.url
            if url == VAULT + "/":
                html = '<button id="arm">Arm selected synthetic item</button>'
            elif url.startswith(("https://accounts.google.com/", "https://lookalike.example.test/")):
                html = '<div id="displayIdentifier">member@example.test</div><input type="password" name="Passwd">'
            else:
                return route.abort()
            route.fulfill(content_type="text/html", body=html, headers={
                "Content-Security-Policy": "default-src 'none'; script-src 'self'; connect-src 'self'; frame-ancestors 'none'",
            })
        browser.route("**/*", fixture)
        vault.goto(VAULT)
        vault.evaluate("""id => {
          window.requests = 0;
          document.querySelector('#arm').onclick = () => {
            window.reply = '';
            const p = chrome.runtime.connect(id, {name:'google-login'});
            window.cancel = () => p.disconnect();
            p.onMessage.addListener(m => {
              window.reply = m.status;
              if (m.status === 'request') {
                window.requests++;
                if (!window.hold) p.postMessage({kind:'credential',value:'synthetic-password-for-autofill-test'});
                else window.release = () => p.postMessage({kind:'credential',value:'synthetic-password-for-autofill-test'});
              }
            });
            p.onDisconnect.addListener(() => { void chrome.runtime.lastError; });
            p.postMessage({kind:'arm',email:'member@example.test'});
          };
        }""", extension_id)

        def arm():
            vault.bring_to_front()
            vault.locator("#arm").click()
            vault.wait_for_function("window.reply === 'armed'")
            google.bring_to_front()
            google.locator("body").click(position={"x": 1, "y": 1})

        def toolbar():
            subprocess.run(["xdotool", "key", "--clearmodifiers", "alt+shift+u"], check=True)

        for name, url, modification in [
            ("correct account", GOOGLE, ""),
            ("different account", GOOGLE, 'document.querySelector("#displayIdentifier").textContent="another@example.test"'),
            ("similar domain", "https://lookalike.example.test/v3/signin/challenge/pwd", ""),
            ("verification step", GOOGLE.replace("/pwd", "/totp"), ""),
            ("ambiguous fields", GOOGLE, 'document.body.appendChild(document.querySelector("input").cloneNode())'),
        ]:
            google.goto(url)
            if modification: google.evaluate(modification)
            before = vault.evaluate("window.requests")
            arm()
            expect(google.locator("input").first).to_have_value("")
            toolbar()
            valid = name == "correct account"
            vault.wait_for_function("s => window.reply === s", arg="filled" if valid else "stopped")
            expect(google.locator("input").first).to_have_value(VALUE if valid else "")
            assert vault.evaluate("window.requests") == before + int(valid)
            if valid:
                toolbar()
                assert vault.evaluate("window.requests") == before + 1
            print(name, "passed", flush=True)
        google.goto(GOOGLE)
        vault.evaluate("window.hold = true")
        arm()
        toolbar()
        vault.wait_for_function("window.reply === 'request'")
        google.reload()
        vault.evaluate("window.release()")
        vault.wait_for_function("window.reply === 'stopped'")
        expect(google.locator("input")).to_have_value("")
        print("document replaced before fill passed", flush=True)
        browser.close()


if __name__ == "__main__":
    main()
