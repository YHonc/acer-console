"""Render the real Svelte app (vite preview, no Tauri backend) and check every page."""
import json
import pathlib
import sys

from playwright.sync_api import sync_playwright

HERE = pathlib.Path(__file__).parent
OUT = HERE / "shots-app"
OUT.mkdir(exist_ok=True)
BASE = sys.argv[1] if len(sys.argv) > 1 else "http://127.0.0.1:4173/"

NAV = [("overview", None), ("fan", None), ("settings", "键盘灯光"), ("settings", "硬件辅助与电源"),
       ("settings", "应用与系统"), ("acer", None), ("dc", None)]
LABEL = {"overview": "设备总览", "fan": "性能与风扇", "settings": "设备设置", "acer": "Acer 服务保护", "dc": "Device Center 外设"}


def main() -> int:
    bad = []
    with sync_playwright() as p:
        browser = p.chromium.launch(channel="msedge")
        for theme in ("light", "dark"):
            for w, h in ((1280, 800), (1024, 768)):
                ctx = browser.new_context(viewport={"width": w, "height": h}, color_scheme=theme)
                page = ctx.new_page()
                errors = []
                page.on("pageerror", lambda e: errors.append(str(e)))
                page.goto(BASE)
                page.wait_for_timeout(1500)
                for pg, sub in NAV:
                    page.get_by_role("button", name=LABEL[pg]).first.click()
                    page.wait_for_timeout(700)
                    if sub:
                        page.get_by_role("radio", name=sub).or_(page.get_by_role("tab", name=sub)).first.click()
                        page.wait_for_timeout(700)
                    info = page.evaluate("""() => {
                      const m = document.querySelector('main') || document.scrollingElement;
                      return {sw: m.scrollWidth, cw: m.clientWidth, docW: document.documentElement.scrollWidth,
                              fake: /\\d{2,3}\\s*°C/.test(document.body.innerText)};
                    }""")
                    name = f"{pg}-{(sub or 'main').replace(' ', '')}-{theme}-{w}"
                    page.screenshot(path=str(OUT / f"{name}.png"))
                    ok = info["sw"] <= info["cw"] + 1 and info["docW"] <= w and not info["fake"]
                    print("OK  " if ok else "FAIL", name, json.dumps(info))
                    if not ok:
                        bad.append(name)
                print("pageerrors", theme, w, errors[:5])
                if errors:
                    bad.append(f"errors-{theme}-{w}")
                ctx.close()
        browser.close()
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
