"""Render preview.html in Chromium, capture screenshots and basic layout checks."""
import json
import pathlib
import sys

from playwright.sync_api import sync_playwright

HERE = pathlib.Path(__file__).parent
OUT = HERE / "shots"
OUT.mkdir(exist_ok=True)
URL = (HERE / "preview.html").as_uri()

CASES = [
    ("overview", {}, "light", (1280, 800)),
    ("fan", {}, "light", (1280, 800)),
    ("settings", {"tab": "light"}, "light", (1280, 800)),
    ("settings", {"tab": "aux"}, "dark", (1280, 800)),
    ("settings", {"tab": "app"}, "light", (1024, 768)),
    ("plugin", {"plugin": "acer"}, "dark", (1024, 768)),
    ("plugin", {"plugin": "devices"}, "light", (1024, 768)),
    ("overview", {"scn": "stale"}, "dark", (1024, 768)),
    ("fan", {"scn": "auth"}, "light", (1024, 768)),
    ("overview", {"scn": "offline"}, "light", (860, 700)),
]


def main() -> int:
    problems = []
    with sync_playwright() as p:
        browser = p.chromium.launch(channel="msedge")
        for page_id, extra, theme, (w, h) in CASES:
            ctx = browser.new_context(viewport={"width": w, "height": h}, device_scale_factor=1)
            page = ctx.new_page()
            errors = []
            page.on("pageerror", lambda e: errors.append(str(e)))
            page.on("console", lambda m: errors.append(m.text) if m.type == "error" else None)
            q = {"page": page_id, "theme": theme, **extra}
            page.goto(URL + "?" + "&".join(f"{k}={v}" for k, v in q.items()))
            page.wait_for_timeout(1500)
            info = page.evaluate(
                """() => {
                const m = document.querySelector('#main');
                const over = [...document.querySelectorAll('.page.active *')].filter(e => {
                  const r = e.getBoundingClientRect();
                  return r.width && r.right > m.getBoundingClientRect().right + 1;
                }).slice(0, 5).map(e => e.className || e.tagName);
                return {sw: m.scrollWidth, cw: m.clientWidth, over, docW: document.documentElement.scrollWidth};
                }"""
            )
            name = f"{page_id}-{extra.get('tab') or extra.get('plugin') or extra.get('scn') or 'main'}-{theme}-{w}"
            page.screenshot(path=str(OUT / f"{name}.png"))
            ok = info["sw"] <= info["cw"] and info["docW"] <= w and not info["over"] and not errors
            print(("OK  " if ok else "FAIL"), name, json.dumps(info, ensure_ascii=False), errors[:3])
            if not ok:
                problems.append(name)
            ctx.close()
        browser.close()
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
