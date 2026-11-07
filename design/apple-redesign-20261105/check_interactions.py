"""Click through the main preview flows and report failures."""
import pathlib
import sys

from playwright.sync_api import sync_playwright, expect

HERE = pathlib.Path(__file__).parent
URL = (HERE / "preview.html").as_uri()


def main() -> int:
    errors: list[str] = []
    with sync_playwright() as p:
        browser = p.chromium.launch(channel="msedge")
        page = browser.new_page(viewport={"width": 1280, "height": 800})
        page.on("pageerror", lambda e: errors.append(str(e)))
        page.goto(URL + "?page=overview")
        page.wait_for_timeout(600)

        page.click("#nav button[data-page=fan]")
        page.wait_for_timeout(500)
        expect(page.locator("#page-title")).to_have_text("性能与风扇")
        page.click(".seg[data-seg=profile] [data-v=turbo]")
        expect(page.locator("#profile-tag")).to_have_text("待应用 · 极速")
        expect(page.locator("#profile-apply")).to_be_enabled()
        page.wait_for_timeout(2700)
        expect(page.locator("#profile-tag")).to_have_text("待应用 · 极速")
        page.click("#profile-apply")
        expect(page.locator("#profile-tag")).to_have_text("当前 · 极速", timeout=3000)
        expect(page.locator(".toast").first).to_be_visible()

        page.click(".seg[data-seg=fanmode] [data-v=custom]")
        page.wait_for_timeout(600)
        assert page.locator("#fan-custom").evaluate("e => e.classList.contains('open')")
        page.fill("#s-cpu", "80")
        page.dispatch_event("#s-cpu", "input")
        expect(page.locator("#fan-hint")).to_contain_text("CPU 80%")

        page.click("#pt-add")
        expect(page.locator(".pt-row")).to_have_count(5)
        page.locator(".pt-row input").first.fill("")
        expect(page.locator("#th-start")).to_be_disabled()
        page.locator(".pt-row input").first.fill("45")
        expect(page.locator("#th-start")).to_be_enabled()
        page.click("#th-start")
        expect(page.locator(".sheet")).to_be_visible()
        page.keyboard.press("Escape")
        page.wait_for_timeout(300)
        expect(page.locator(".sheet")).to_have_count(0)
        page.click("#th-start")
        page.click(".sheet [data-a='1']")
        expect(page.locator("#th-tag")).to_have_text("运行中", timeout=3000)
        expect(page.locator("#fan-apply")).to_be_disabled()
        page.click("#th-stop")
        expect(page.locator("#th-tag")).to_have_text("已停止", timeout=3000)

        page.click("#nav button[data-page=settings]")
        page.wait_for_timeout(500)
        page.click(".fx[data-fx=breath]")
        expect(page.locator("#light-tag")).to_have_text("待应用草稿")
        assert page.locator(".cdot").first.is_enabled()
        page.click(".fx[data-fx=static]")
        page.wait_for_timeout(600)
        assert page.locator("#light-static").evaluate("e => e.classList.contains('open')")
        page.click(".seg[data-seg=settingsTab] [data-v=app]")
        page.wait_for_timeout(500)
        page.click("#pref-tray", force=True)
        expect(page.locator("#pref-save")).to_be_enabled()
        page.click(".disc[aria-controls=cap-panel]")
        page.wait_for_timeout(600)
        expect(page.locator("#cap-list .row")).to_have_count(7)

        page.click("#pv-fab")
        page.click(".seg[data-seg=scn] [data-v=offline]")
        page.wait_for_timeout(400)
        expect(page.locator("#chip-conn")).to_contain_text("已断线")
        expect(page.locator("#light-apply")).to_be_disabled()
        page.click("[data-auth]")
        expect(page.locator("#chip-conn")).to_contain_text("已连接", timeout=3000)

        page.click(".seg[data-seg=ts] [data-v='5']")
        page.click("#pv-rm", force=True)
        page.click("#nav button[data-page=plugin]")
        page.wait_for_timeout(500)
        page.click("#log-sw", force=True)
        page.click("#log-btn")
        expect(page.locator(".popover")).to_be_visible()
        page.screenshot(path=str(HERE / "shots" / "interaction-end.png"))
        browser.close()
    print("pageerrors:", errors)
    print("ALL INTERACTIONS OK" if not errors else "ERRORS")
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main())
