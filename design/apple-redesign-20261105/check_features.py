"""Check the 2026-11-06 additions: overlay scrollbar, collapsible sidebar, non-locking sync, Device Center page."""
import pathlib
import sys

from playwright.sync_api import sync_playwright, expect

HERE = pathlib.Path(__file__).parent
URL = (HERE / "preview.html").as_uri()
SHOTS = HERE / "shots"


def main() -> int:
    errors: list[str] = []
    with sync_playwright() as p:
        browser = p.chromium.launch(channel="msedge")
        ctx = browser.new_context(viewport={"width": 1280, "height": 800})
        page = ctx.new_page()
        page.on("pageerror", lambda e: errors.append(str(e)))
        page.goto(URL + "?page=overview")
        page.wait_for_timeout(700)

        # 1 滚动条：滚动出现，1 秒后淡出；悬停滑块加宽
        main_el = page.locator("#main")
        assert main_el.evaluate("e => e.scrollHeight > e.clientHeight"), "main 不可滚动"
        main_el.evaluate("e => e.scrollTo({top: 300, behavior: 'instant'})")
        page.wait_for_timeout(250)
        sb = page.locator(".sb.on")
        expect(sb).to_have_count(1)
        assert page.locator(".sb-thumb").first.evaluate("e => e.offsetWidth") == 6
        box = sb.bounding_box()
        page.mouse.move(box["x"] + box["width"] / 2, box["y"] + 40)
        page.wait_for_timeout(500)
        assert page.locator(".sb-thumb").first.evaluate("e => e.offsetWidth") == 10
        page.mouse.move(600, 400)
        page.wait_for_timeout(1400)
        expect(page.locator(".sb.on")).to_have_count(0)
        light_c = page.locator(".sb-thumb").first.evaluate("e => getComputedStyle(e).backgroundColor")

        # 2 侧栏：按钮、Ctrl+B、localStorage、悬停提示、窄窗口
        page.click("#side-toggle")
        page.wait_for_timeout(700)
        assert page.evaluate("document.documentElement.dataset.side") == "collapsed"
        assert abs(page.locator("#sidebar").bounding_box()["width"] - 64) < 1
        assert page.evaluate("localStorage.getItem('acer-preview-sidebar')") == "collapsed"
        page.hover("#nav button[data-page=fan]")
        expect(page.locator(".side-tip.on")).to_have_text("性能与风扇", timeout=1500)
        page.screenshot(path=str(SHOTS / "feature-sidebar-collapsed.png"))
        page.reload()
        page.wait_for_timeout(400)
        assert page.evaluate("document.documentElement.dataset.side") == "collapsed"
        page.mouse.move(700, 400)
        page.keyboard.press("Control+b")
        page.wait_for_timeout(700)
        assert page.evaluate("document.documentElement.dataset.side") == "expanded"
        assert abs(page.locator("#sidebar").bounding_box()["width"] - 232) < 1
        page.set_viewport_size({"width": 860, "height": 700})
        page.wait_for_timeout(700)
        assert page.evaluate("document.documentElement.dataset.side") == "collapsed"
        assert page.evaluate("localStorage.getItem('acer-preview-sidebar')") == "expanded"
        page.set_viewport_size({"width": 1280, "height": 800})
        page.wait_for_timeout(700)
        assert page.evaluate("document.documentElement.dataset.side") == "expanded"

        # 3 读取不锁操作：同步中控件可编辑，草稿与读回冲突
        page.click("#nav button[data-page=fan]")
        page.wait_for_timeout(500)
        page.click(".seg[data-seg=profile] [data-v=turbo]")
        page.click("#pv-fab")
        page.click("#pv-ext")
        page.wait_for_timeout(200)
        assert page.locator("#refresh-btn").evaluate("e => e.classList.contains('syncing') && !e.disabled")
        expect(page.locator(".seg[data-seg=profile] [data-v=quiet]")).to_be_enabled()
        expect(page.locator("#profile-apply")).to_be_enabled()
        page.click("#refresh-btn")
        expect(page.locator("#profile-cf")).to_have_class("collapse cf open", timeout=3000)
        expect(page.locator("#profile-cf-t")).to_contain_text("设备已变为")
        expect(page.locator("#profile-tag")).to_have_text("待应用 · 极速")
        page.click("#pv-fab")
        page.click("[data-adopt=profile]")
        expect(page.locator("#profile-tag")).to_contain_text("当前 ·")
        expect(page.locator("#profile-cf")).not_to_have_class("collapse cf open")

        # 4 Device Center 外设
        page.click("#nav button[data-page=dc]")
        page.wait_for_timeout(600)
        expect(page.locator("#page-title")).to_have_text("Device Center 外设")
        expect(page.locator("text=鼠标灯效：暂不支持")).to_have_count(1)
        expect(page.locator("[data-roll='dcBatt-kb']")).to_have_attribute("aria-label", "—")
        expect(page.locator("#dc-token-mask")).to_contain_text("••••")
        page.click(".seg[data-seg=dc-mouse-dpi] [data-v='800']")
        expect(page.locator("#dc-mouse-tag")).to_contain_text("待应用")
        page.click("#pv-fab")
        page.click("#pv-ext")
        expect(page.locator("#dc-cf-mouse")).to_have_class("collapse cf open", timeout=3000)
        expect(page.locator("#dc-cf-mouse")).to_contain_text("设备已变为 DPI")
        dev_dpi = page.locator(".seg[data-seg=dc-mouse-dpi] [data-current]").get_attribute("data-v")
        assert dev_dpi not in (None, "800")
        expect(page.locator("#dc-cf-mouse")).to_contain_text(f"DPI {dev_dpi}")
        assert page.locator(".seg[data-seg=dc-mouse-dpi]").get_attribute("data-value") == "800"
        page.click("#pv-fab")
        page.locator("#dc-cf-mouse").scroll_into_view_if_needed()
        page.wait_for_timeout(300)
        page.screenshot(path=str(SHOTS / "feature-dc-conflict.png"))
        page.click("[data-adopt=mouse]")
        expect(page.locator("#dc-mouse-tag")).to_have_text(f"当前 · {dev_dpi} DPI · 1000 Hz")
        # 单项写入只锁该开关
        page.click("[data-sw=mac] input", force=True)
        assert page.locator("[data-sw=mac]").evaluate("e => e.classList.contains('pending')")
        expect(page.locator("[data-sw=mac] input")).to_be_disabled()
        expect(page.locator("[data-sw=lowLatency] input")).to_be_enabled()
        expect(page.locator(".seg[data-seg=dc-kb-rate] button").first).to_be_enabled()
        page.wait_for_timeout(1100)
        expect(page.locator("[data-sw=mac] input")).to_be_enabled()
        expect(page.locator("[data-unk=winLock]")).to_be_visible()
        page.click(".disc[aria-controls=dc-soon]")
        page.wait_for_timeout(600)
        expect(page.locator("#dc-soon .soon")).to_have_count(4)
        assert page.locator("#dc-soon button, #dc-soon input").count() == 0
        # 模块文件变更与移除
        page.click("#pv-fab")
        page.click("#pv-dchash")
        expect(page.locator("#dc-mod-tag")).to_have_text("待确认")
        expect(page.locator("#dc-mouse-apply")).to_be_disabled()
        page.click("#pv-fab")
        page.wait_for_timeout(600)
        page.locator("#dc-trust").evaluate("e => e.scrollIntoView({block: 'center'})")
        page.wait_for_timeout(300)
        page.click("#dc-trust")
        expect(page.locator("#dc-mod-tag")).to_have_text("运行中", timeout=3000)
        page.click("#pv-fab")
        page.click("#pv-dcmod", force=True)
        page.wait_for_timeout(500)
        expect(page.locator("#page-title")).to_have_text("Acer 服务保护")
        assert page.locator("#nav-dc").evaluate("e => e.classList.contains('gone') && e.inert")
        assert page.locator("#nav-acer").evaluate("e => !e.classList.contains('gone') && e.getBoundingClientRect().height > 30")
        page.click("#pv-dcmod", force=True)
        page.wait_for_timeout(500)
        assert page.locator("#nav-dc").evaluate("e => !e.classList.contains('gone') && e.getBoundingClientRect().height > 30")
        page.click("#pv-fab")

        # 深色滚动条与减少动态效果
        page.click("#theme-btn")
        page.wait_for_timeout(800)
        main_el.evaluate("e => e.scrollTo({top: 200, behavior: 'instant'})")
        page.wait_for_timeout(250)
        dark_c = page.locator(".sb-thumb").first.evaluate("e => getComputedStyle(e).backgroundColor")
        assert dark_c != light_c, (dark_c, light_c)
        page.screenshot(path=str(SHOTS / "feature-scrollbar-dark.png"))
        ctx.close()

        ctx = browser.new_context(viewport={"width": 1280, "height": 800}, reduced_motion="reduce")
        page = ctx.new_page()
        page.on("pageerror", lambda e: errors.append(str(e)))
        page.goto(URL + "?page=overview")
        page.wait_for_timeout(400)
        page.click("#side-toggle")
        page.wait_for_timeout(60)
        assert abs(page.locator("#sidebar").bounding_box()["width"] - 64) < 1, "减少动态效果时应直接切换"
        # 减少动效：键盘换位不产生动画
        page.evaluate("localStorage.removeItem('acer-preview-dev-order')")
        page.goto(URL + "?page=dc")
        page.wait_for_timeout(400)
        page.focus(".dev-row[data-id=mouse] .grip")
        page.keyboard.press("ArrowDown")
        assert page.evaluate("document.getAnimations().filter(a => !(a instanceof CSSAnimation) && !(a instanceof CSSTransition) && a.effect?.target?.classList?.contains('dev-row')).length") == 0
        assert order(page) == ["kb", "mouse", "lamp"]
        ctx.close()

        errors += split_and_order(browser) + popup_buttons(browser)
        browser.close()
    print("pageerrors:", errors)
    print("ALL FEATURES OK" if not errors else "ERRORS")
    return 1 if errors else 0


def order(page):
    return page.eval_on_selector_all("#dc-devs .dev-row", "rs => rs.map(r => r.dataset.id)")


def sec_order(page):
    return page.eval_on_selector_all("#dc-secs > .dc-sec", "rs => rs.map(r => r.dataset.sec)")


def split_and_order(browser) -> list[str]:
    errors: list[str] = []
    ctx = browser.new_context(viewport={"width": 1280, "height": 800})
    page = ctx.new_page()
    page.on("pageerror", lambda e: errors.append(str(e)))
    page.goto(URL + "?page=overview")
    page.wait_for_timeout(500)

    # 1 服务插件拆分：父项进入第一个子页；两个子入口平级
    page.click("#nav button[data-page=plugin]")
    page.wait_for_timeout(500)
    expect(page.locator("#page-title")).to_have_text("Acer 服务保护")
    assert page.locator("#nav-acer").get_attribute("aria-current") == "page"
    expect(page.locator(".seg[data-seg=pluginTab]")).to_have_count(0)
    page.click("#nav-dc")
    page.wait_for_timeout(500)
    expect(page.locator("#page-title")).to_have_text("Device Center 外设")
    page.click("#side-toggle")
    page.wait_for_timeout(700)
    for sel, text in (("#nav-acer", "Acer 服务保护"), ("#nav-dc", "Device Center 外设")):
        assert page.locator(sel + " .sq").is_visible()
        page.hover(sel)
        expect(page.locator(".side-tip.on")).to_have_text(text, timeout=1500)
    page.mouse.move(700, 400)
    page.click("#side-toggle")
    page.wait_for_timeout(700)

    # 2 设备顺序：键盘
    assert order(page) == ["mouse", "kb", "lamp"] and sec_order(page) == ["mouse", "kb", "lamp"]
    page.focus(".dev-row[data-id=mouse] .grip")
    page.keyboard.press("ArrowDown")
    page.keyboard.press("ArrowDown")
    assert order(page) == ["kb", "lamp", "mouse"], order(page)
    assert sec_order(page) == ["kb", "lamp", "mouse"]
    assert page.evaluate("document.activeElement.closest('.dev-row').dataset.id") == "mouse"
    expect(page.locator("#dc-live")).to_contain_text("第 3 位")
    # 拖动：把挂灯拖到最上
    page.wait_for_timeout(450)  # 等键盘换位的 snappy 位移动画（370ms）结束，否则把手位置还在移动
    grip = page.locator(".dev-row[data-id=lamp] .grip").bounding_box()
    first = page.locator(".dev-row[data-id=kb]").bounding_box()
    page.mouse.move(grip["x"] + grip["width"] / 2, grip["y"] + grip["height"] / 2)
    page.mouse.down()
    for k in range(1, 9):
        page.mouse.move(grip["x"] + grip["width"] / 2, grip["y"] + grip["height"] / 2 - (grip["y"] - first["y"] + 10) * k / 8)
        page.wait_for_timeout(16)
    assert page.locator(".dev-row[data-id=lamp]").evaluate("e => e.classList.contains('dragging')")
    page.mouse.up()
    page.wait_for_timeout(500)
    assert order(page) == ["lamp", "kb", "mouse"], order(page)
    assert sec_order(page) == ["lamp", "kb", "mouse"]
    assert page.evaluate("JSON.parse(localStorage.getItem('acer-preview-dev-order'))") == ["lamp", "kb", "mouse"]
    page.goto(URL + "?page=dc")
    page.wait_for_timeout(500)
    assert order(page) == ["lamp", "kb", "mouse"] and sec_order(page) == ["lamp", "kb", "mouse"]
    # 断开保留原位、新设备追加到末尾
    page.click("#pv-fab")
    page.click("#pv-kb", force=True)
    expect(page.locator("[data-conn=kb]")).to_contain_text("已断开")
    assert order(page) == ["lamp", "kb", "mouse"]
    expect(page.locator("#dc-kb-tag")).to_have_text("设备已断开")
    expect(page.locator("[data-sw=mac]")).to_be_hidden()
    expect(page.locator("[data-unk=mac]")).to_have_text("—")
    expect(page.locator("#dc-mouse-tag")).to_contain_text("当前")
    page.click("#pv-newdev")
    page.wait_for_timeout(600)
    assert order(page) == ["lamp", "kb", "mouse", "pad"], order(page)
    page.click("#pv-fab")
    page.locator("#dc-devs").scroll_into_view_if_needed()
    page.wait_for_timeout(400)
    page.screenshot(path=str(SHOTS / "feature-dev-order.png"))
    page.click("#pv-fab")
    page.click("#pv-dcmod", force=True)
    page.wait_for_timeout(500)
    assert page.locator("#nav-acer").is_visible()
    assert page.locator("#nav-dc").evaluate("e => e.classList.contains('gone')")
    ctx.close()
    return errors


def popup_buttons(browser) -> list[str]:
    errors: list[str] = []
    for theme in ("light", "dark"):
        ctx = browser.new_context(viewport={"width": 1280, "height": 800})
        page = ctx.new_page()
        page.on("pageerror", lambda e: errors.append(str(e)))
        page.goto(URL + f"?page=settings&tab=aux&theme={theme}")
        page.wait_for_timeout(600)
        n_sel = page.locator("select:not(#pv-panel select)").count()
        assert n_sel == 7 and page.locator(".popbtn:not(#pv-panel .popbtn)").count() == n_sel
        assert page.evaluate("[...document.querySelectorAll('select')].every(s => s.hidden && s.nextElementSibling.classList.contains('popbtn'))")
        btn = page.locator("#pop-rule-ac")
        expect(btn).to_have_attribute("aria-haspopup", "listbox")
        expect(btn).to_contain_text("性能")
        btn.click()
        menu = page.locator(".popmenu")
        expect(menu).to_be_visible()
        expect(btn).to_have_attribute("aria-expanded", "true")
        expect(menu).to_have_attribute("role", "listbox")
        expect(menu.locator("[aria-selected=true]")).to_have_text("性能")
        assert menu.get_attribute("data-place") == "align"
        sel_box = menu.locator("[aria-selected=true]").bounding_box()
        b = btn.bounding_box()
        assert abs((sel_box["y"] + sel_box["height"] / 2) - (b["y"] + b["height"] / 2)) < 2
        if theme == "light":
            page.screenshot(path=str(SHOTS / "feature-popup-light.png"))
        bg = menu.evaluate("e => getComputedStyle(e).backgroundColor")
        assert "blur" in menu.evaluate("e => getComputedStyle(e).backdropFilter")
        # 键盘：↓ + Enter
        page.keyboard.press("ArrowDown")
        page.keyboard.press("Enter")
        page.wait_for_timeout(250)
        expect(page.locator(".popmenu")).to_have_count(0)
        expect(btn).to_contain_text("极速")
        assert page.locator("#rule-ac").input_value() == "turbo"
        expect(page.locator("#rule-tag")).to_have_text("待保存草稿")
        assert page.evaluate("document.activeElement.id") == "pop-rule-ac"
        # 首字母跳转与 Esc
        page.keyboard.press("Enter")
        page.keyboard.press("a")
        expect(page.locator(".pop-opt.active")).to_have_text("安静")
        page.keyboard.press("Escape")
        page.wait_for_timeout(250)
        expect(page.locator(".popmenu")).to_have_count(0)
        assert page.locator("#rule-ac").input_value() == "turbo"
        # 点击外部关闭
        page.click("#pop-audio")
        expect(page.locator(".popmenu:not(.out) .pop-cur")).to_have_count(1)
        expect(page.locator(".popmenu:not(.out) .pop-opt:has(.pop-cur)")).to_contain_text("自动")
        page.mouse.click(60, 700)
        page.wait_for_timeout(250)
        expect(page.locator(".popmenu")).to_have_count(0)
        if theme == "dark":
            page.click("#pop-audio")
            page.wait_for_timeout(450)
            page.screenshot(path=str(SHOTS / "feature-popup-dark.png"))
            page.keyboard.press("Escape")
            assert bg != light_bg, (bg, light_bg)
        else:
            light_bg = bg
        # 窗口底部空间不足时向下展开
        page.set_viewport_size({"width": 1280, "height": 420})
        page.goto(URL + f"?page=settings&tab=app&theme={theme}")
        page.wait_for_timeout(600)
        page.locator("#pop-poll").evaluate("e => e.scrollIntoView({block: 'start'})")
        page.evaluate("document.querySelector('#main').scrollBy(0, -60)")
        page.wait_for_timeout(200)
        page.click("#pop-poll")
        place = page.locator(".popmenu").get_attribute("data-place")
        page.keyboard.press("Escape")
        assert place in ("align", "below")
        page.set_viewport_size({"width": 1280, "height": 800})
        page.goto(URL + "?page=dc")
        page.wait_for_timeout(600)
        # 外设：橙点标当前值；设备断开时显示“—”
        kbl = page.locator(".popbtn[aria-label=键盘灯效模式]")
        kbl.click()
        expect(page.locator(".popmenu:not(.out) .pop-opt:has(.pop-cur)")).to_contain_text("波浪")
        page.locator(".popmenu:not(.out) .pop-opt", has_text="呼吸").click()
        expect(page.locator("#dc-kbl-tag")).to_contain_text("待应用")
        kbl.click()
        expect(page.locator(".popmenu:not(.out) .pop-opt:has(.pop-cur)")).to_contain_text("波浪")
        expect(page.locator(".popmenu:not(.out) [aria-selected=true]")).to_contain_text("呼吸")
        page.keyboard.press("Escape")
        page.click("#pv-fab")
        page.click("#pv-kb", force=True)
        page.click("#pv-fab")
        sleep_btn = page.locator(".popbtn[aria-label=休眠时间]")
        expect(sleep_btn).to_be_disabled()
        expect(sleep_btn.locator(".pop-val > .on")).to_have_text("—")
        ctx.close()
    return errors


if __name__ == "__main__":
    sys.exit(main())
