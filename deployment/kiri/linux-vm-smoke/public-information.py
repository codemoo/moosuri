#!/usr/bin/env python3
"""Public page browser acceptance against built assets, without member/provider data.

Gateway host/origin/method guards have a separate Rust route test. This harness
only serves the three public page entries and local assets; it never impersonates
Google, a model, or an authenticated member service.
"""
import argparse
from functools import partial
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
import sys
from threading import Thread
from urllib.parse import urlsplit

from playwright.sync_api import expect, sync_playwright


class Assets(SimpleHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def do_GET(self):
        if urlsplit(self.path).path in ("/about", "/privacy", "/terms", "/app"):
            self.path = "/personal.html"
        super().do_GET()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--dist", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    args = parser.parse_args()
    assert sys.platform == "linux", "run browser acceptance on Linux"
    assert (args.dist / "personal.html").is_file(), "build web assets first"
    args.output_dir.mkdir(mode=0o700, parents=True, exist_ok=True)
    server = ThreadingHTTPServer(("127.0.0.1", 0), partial(Assets, directory=str(args.dist)))
    Thread(target=server.serve_forever, daemon=True).start()
    origin = f"http://127.0.0.1:{server.server_port}"
    try:
        with sync_playwright() as pw:
            browser = pw.chromium.launch(channel="chromium", headless=True)
            try:
                for width in (390, 1280):
                    context = browser.new_context(viewport={"width": width, "height": 900})
                    page = context.new_page()
                    requests, errors = [], []
                    page.on("request", lambda request: requests.append(request.url))
                    page.on("pageerror", lambda error: errors.append(str(error)))
                    for path, title in (("about", "Moosuri 소개"), ("privacy", "개인정보 처리방침"), ("terms", "이용 약관")):
                        page.goto(origin + "/" + path)
                        expect(page.get_by_role("heading", name=title, level=1)).to_be_visible()
                        expect(page.get_by_role("navigation", name="공개 정보")).to_be_visible()
                        assert page.evaluate("document.documentElement.scrollWidth <= innerWidth"), "horizontal_overflow"
                        assert page.get_by_role("main").aria_snapshot(), "empty_public_content"
                        page.screenshot(path=str(args.output_dir / f"{path}-{width}.png"), full_page=True)
                    page.get_by_role("navigation", name="공개 정보").get_by_role("link", name="소개", exact=True).click()
                    expect(page.get_by_role("heading", name="Moosuri 소개", level=1)).to_be_visible()
                    page.get_by_role("main").get_by_role("link", name="개인정보 처리방침", exact=True).click()
                    expect(page).to_have_url(origin + "/privacy")
                    page.reload()
                    expect(page.get_by_role("heading", name="개인정보 처리방침", level=1)).to_be_visible()
                    page.keyboard.press("Tab")
                    assert page.locator(":focus").evaluate("e => e.tagName") == "A", "keyboard_link_missing"
                    assert not errors, "browser_execution_error"
                    assert all(url.startswith(origin + "/") for url in requests), "external_request"
                    assert not any("/api/" in url or "PersonalPreview-" in url for url in requests), "member_application_loaded"
                    assert not context.cookies(), "public_page_set_cookie"
                    context.close()
                    print(f"PASS public information: direct entries, links/reload, keyboard/layout, no member/provider requests ({width}px)", flush=True)
            finally:
                browser.close()
    finally:
        server.shutdown()
        server.server_close()


if __name__ == "__main__":
    main()
