"""检查 Device Center 模块状态（契约 v1.0.2 第 7 节）在预览中的表现，以及退出时后台保持失败的弹窗。"""
import pathlib
import sys

from playwright.sync_api import sync_playwright, expect

HERE = pathlib.Path(__file__).parent
URL = (HERE / "preview.html").as_uri()
SHOTS = HERE / "shots"
STATES = {
    "starting": ("启动中", "正在启动模块"),
    "reconnecting": ("重连中", "正在重连（第 2/3 次）"),
    "stopped": ("已停止", "模块未运行"),
    "elsewhere": ("被占用", "已在其他地方运行"),
    "incompatible": ("不兼容", "协议版本不兼容"),
    "error": ("异常", "模块异常退出"),
}


def main() -> int:
    fails: list[str] = []
    with sync_playwright() as p:
        browser = p.chromium.launch(channel="msedge")
        for theme in ("light", "dark"):
            ctx = browser.new_context(viewport={"width": 1280, "height": 800})
            page = ctx.new_page()
            errs: list[str] = []
            page.on("pageerror", lambda e: errs.append(str(e)))
            page.goto(URL + f"?page=dc&theme={theme}")
            page.wait_for_timeout(700)
            for state, (tag, note) in STATES.items():
                page.evaluate("v => { const s = document.querySelector('#pv-dcstate'); s.value = v; s.dispatchEvent(new Event('change')); }", state)
                page.wait_for_timeout(450)
                t = f"[{theme} {state}]"
                if page.locator("#dc-mod-tag").inner_text() != tag:
                    fails.append(f"{t} 标签应为 {tag}")
                if "open" not in (page.locator("#dc-note").get_attribute("class") or "") or note not in page.locator("#dc-note").inner_text():
                    fails.append(f"{t} 状态行未显示“{note}”")
                if page.locator("#dc-mouse-tag").inner_text() != "状态未知":
                    fails.append(f"{t} 非运行状态下鼠标仍显示读数")
                if page.locator("[data-roll='dcBatt-mouse']").get_attribute("aria-label") != "—":
                    fails.append(f"{t} 非运行状态下电量不是 —")
                if not page.locator("#dc-mouse-apply").is_disabled():
                    fails.append(f"{t} 非运行状态下应用按钮可用")
                if page.evaluate("document.documentElement.scrollWidth > innerWidth"):
                    fails.append(f"{t} 横向溢出")
                if theme == "light" and state in ("reconnecting", "error"):
                    page.locator("#main").evaluate("e => e.scrollTo({top: 0, behavior: 'instant'})")
                    page.wait_for_timeout(300)
                    page.screenshot(path=str(SHOTS / f"feature-dc-{state}.png"), clip={"x": 232, "y": 0, "width": 1048, "height": 560})
            # 恢复运行：通过“启动模块”按钮
            page.evaluate("v => { const s = document.querySelector('#pv-dcstate'); s.value = v; s.dispatchEvent(new Event('change')); }", "stopped")
            page.wait_for_timeout(450)
            page.click("[data-mod=start]")
            expect(page.locator("#dc-mod-tag")).to_have_text("运行中", timeout=3000)
            page.wait_for_timeout(450)
            if "open" in (page.locator("#dc-note").get_attribute("class") or ""):
                fails.append(f"[{theme}] 运行后状态行未收起")
            if not page.locator("#dc-mouse-tag").inner_text().startswith("当前"):
                fails.append(f"[{theme}] 运行后鼠标未恢复读回")
            # 退出失败弹窗：取消保持运行
            page.click("#pv-fab")
            page.click("#pv-exitfail")
            dlg = page.locator(".sheet[role=alertdialog]")
            expect(dlg).to_contain_text("未能让模块留在后台")
            if theme == "light":
                page.wait_for_timeout(400)
                page.screenshot(path=str(SHOTS / "feature-dc-exitfail.png"))
            page.keyboard.press("Escape")
            expect(dlg).to_have_count(0)
            if errs:
                fails.append(f"[{theme}] 页面错误 {errs[:2]}")
            ctx.close()
        browser.close()
    for f in fails:
        print("FAIL", f)
    print("PASS" if not fails else f"{len(fails)} 项失败")
    return 1 if fails else 0


if __name__ == "__main__":
    sys.exit(main())
