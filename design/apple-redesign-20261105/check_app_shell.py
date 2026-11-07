"""正式前端（vite preview，无 Tauri 后端）：阶段 2 外壳检查——覆盖式滚动条与侧栏收起。

先在 apps/desktop 运行 `npx vite preview --host 127.0.0.1 --port 4173`。
"""
import pathlib
import sys

from playwright.sync_api import sync_playwright, expect

HERE = pathlib.Path(__file__).parent
OUT = HERE / "shots-app"
BASE = sys.argv[1] if len(sys.argv) > 1 else "http://127.0.0.1:4173/"
KEY = "{{SECRET_ptuqvxuu}}"


def check(cond, msg, fails):
    if not cond:
        fails.append(msg)


def main() -> int:
    fails: list[str] = []
    with sync_playwright() as p:
        browser = p.chromium.launch(channel="msedge")
        for theme in ("light", "dark"):
            ctx = browser.new_context(viewport={"width": 1280, "height": 800}, color_scheme=theme)
            page = ctx.new_page()
            errs: list[str] = []
            page.on("pageerror", lambda e: errs.append(str(e)))
            page.goto(BASE)
            page.wait_for_timeout(1500)
            t = f"[{theme}]"
            # 原生滚动条隐藏
            check(page.evaluate("getComputedStyle(document.querySelector('main')).scrollbarWidth") == "none", f"{t} 原生滚动条未隐藏", fails)
            # 去设置页（内容最长）
            page.get_by_role("button", name="设备设置").first.click()
            page.wait_for_timeout(800)
            main = page.locator("main")
            scrollable = main.evaluate("e => e.scrollHeight > e.clientHeight + 1")
            check(scrollable, f"{t} 设置页 main 不可滚动，无法检查滚动条", fails)
            if scrollable:
                w0 = main.evaluate("e => e.clientWidth")
                check(page.locator(".sb.on").count() == 0, f"{t} 切页后滚动条不应出现", fails)
                main.evaluate("e => e.scrollTo({top: 240, behavior: 'instant'})")
                page.wait_for_timeout(250)
                check(page.locator(".sb.on").count() == 1, f"{t} 滚动后滚动条未出现", fails)
                check(main.evaluate("e => e.clientWidth") == w0, f"{t} 滚动条改变了内容宽度", fails)
                sb = page.locator(".sb.on").bounding_box()
                mb = main.bounding_box()
                check(sb and sb["y"] >= mb["y"] + 50, f"{t} 滚动条与工具栏重叠", fails)
                thumb = page.locator(".sb.on .sb-thumb")
                check(thumb.evaluate("e => e.offsetWidth") == 6, f"{t} 滑块初始宽度不是 6px", fails)
                page.mouse.move(sb["x"] + sb["width"] / 2, sb["y"] + 40)
                page.wait_for_timeout(500)
                check(thumb.evaluate("e => e.offsetWidth") == 10, f"{t} 悬停后滑块未加宽", fails)
                page.mouse.move(sb["x"] + sb["width"] / 2, sb["y"] + sb["height"] - 6)
                page.mouse.down(); page.mouse.up()
                page.wait_for_timeout(150)
                st, mx = main.evaluate("e => [e.scrollTop, e.scrollHeight - e.clientHeight]")
                check(st > mx * 0.8, f"{t} 点击轨道未跳转 {st}/{mx}", fails)
                page.mouse.move(640, 400)
                page.wait_for_timeout(1500)
                check(page.locator(".sb.on").count() == 0, f"{t} 停止后滚动条未淡出", fails)

            # 侧栏
            side = page.locator("#sidebar")
            check(abs(side.bounding_box()["width"] - 232) < 1, f"{t} 侧栏初始宽度不是 232", fails)
            page.click(".side-toggle")
            page.wait_for_timeout(120)
            check(page.evaluate("[...document.querySelectorAll('.nav button')].every(b => b.offsetHeight <= 37)"), f"{t} 收起过渡中导航挤压", fails)
            page.wait_for_timeout(700)
            check(page.evaluate("document.documentElement.dataset.side") == "collapsed", f"{t} 未收起", fails)
            check(abs(side.bounding_box()["width"] - 64) < 1, f"{t} 收起宽度不是 64", fails)
            check(page.evaluate(f"localStorage.getItem('{KEY}')") == "collapsed", f"{t} 未记住收起", fails)
            check(page.locator(".side-toggle").get_attribute("aria-expanded") == "false", f"{t} aria-expanded 未更新", fails)
            page.get_by_role("button", name="性能与风扇").first.hover()
            expect(page.locator(".side-tip")).to_have_text("性能与风扇", timeout=1500)
            page.locator(".foot-mini").hover()
            expect(page.locator(".side-tip")).to_contain_text(" · ", timeout=1500)
            page.mouse.move(700, 400)
            page.wait_for_timeout(200)
            check(page.locator(".side-tip").count() == 0, f"{t} 提示未消失", fails)
            if theme == "light":
                page.screenshot(path=str(OUT / "shell-collapsed-light.png"))
            page.reload()
            page.wait_for_timeout(1200)
            check(page.evaluate("document.documentElement.dataset.side") == "collapsed", f"{t} 刷新后未保持收起", fails)
            page.keyboard.press("Control+b")
            page.wait_for_timeout(700)
            check(abs(side.bounding_box()["width"] - 232) < 1, f"{t} Ctrl+B 未展开", fails)
            page.get_by_role("button", name="性能与风扇").first.hover()
            page.wait_for_timeout(600)
            check(page.locator(".side-tip").count() == 0, f"{t} 展开时导航不应显示提示", fails)
            # 窄窗口自动收起，不改偏好
            page.set_viewport_size({"width": 860, "height": 700})
            page.wait_for_timeout(700)
            check(page.evaluate("document.documentElement.dataset.side") == "collapsed", f"{t} 窄窗口未自动收起", fails)
            check(page.evaluate(f"localStorage.getItem('{KEY}')") == "expanded", f"{t} 窄窗口改了偏好", fails)
            check(page.evaluate("document.documentElement.scrollWidth <= innerWidth"), f"{t} 窄窗口横向溢出", fails)
            if theme == "dark":
                page.screenshot(path=str(OUT / "shell-narrow-dark.png"))
            page.set_viewport_size({"width": 1280, "height": 800})
            page.wait_for_timeout(700)
            check(page.evaluate("document.documentElement.dataset.side") == "expanded", f"{t} 恢复宽度后未展开", fails)
            # 静止无动画
            page.wait_for_timeout(1500)
            running = page.evaluate("document.getAnimations().filter(a => a.playState === 'running').length")
            check(running == 0, f"{t} 静止时仍有 {running} 个动画", fails)
            check(not errs, f"{t} 页面错误 {errs[:2]}", fails)
            ctx.close()
        # 减少动效
        ctx = browser.new_context(viewport={"width": 1280, "height": 800}, reduced_motion="reduce")
        page = ctx.new_page()
        page.goto(BASE)
        page.wait_for_timeout(1200)
        page.click(".side-toggle")
        page.wait_for_timeout(60)
        check(abs(page.locator("#sidebar").bounding_box()["width"] - 64) < 1, "减少动效下侧栏仍在过渡", fails)
        ctx.close()
        browser.close()
    for f in fails:
        print("FAIL", f)
    print("PASS" if not fails else f"{len(fails)} 项失败")
    return 1 if fails else 0


if __name__ == "__main__":
    sys.exit(main())
