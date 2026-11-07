"""补齐 2026-11-06 浏览器检查中未验证的项（work/browser-check-preview-20261106.md）。

check_features.py 已覆盖：菜单深浅色键盘/外部点击、拖动排序、单开关写入锁定。
本脚本补：滚动条交互与布局跳动、侧栏过渡文字、冲突两按钮最终状态、同步重复点击、
外设二期折叠与哈希确认、四种组合逐页溢出/截字/静止动画、系统减少动效。
"""
import pathlib
import sys

from playwright.sync_api import sync_playwright, expect

HERE = pathlib.Path(__file__).parent
URL = (HERE / "preview.html").as_uri()
SHOTS = HERE / "shots"

PAGES = [
    ("overview", ""), ("fan", ""), ("settings", "&tab=light"), ("settings", "&tab=aux"),
    ("settings", "&tab=app"), ("acer", ""), ("dc", ""),
]

# 视觉上被裁掉的文字：可见、有文本、overflow 非 visible、内容宽于盒子且不是省略号设计
CLIPPED_JS = """() => {
  const out = [];
  for (const el of document.querySelectorAll('#main *')) {
    if (!el.offsetParent || el.closest('.collapse:not(.open), [hidden], .sb, .roll')) continue;
    if (el.children.length && [...el.childNodes].every(n => n.nodeType !== 3 || !n.textContent.trim())) continue;
    if (!el.textContent.trim()) continue;
    const cs = getComputedStyle(el);
    if (cs.overflowX === 'visible' || cs.textOverflow === 'ellipsis') continue;
    if (el.scrollWidth > el.clientWidth + 1 && el.clientWidth > 0)
      out.push((el.id ? '#' + el.id : el.tagName.toLowerCase() + '.' + el.className) + ' :: ' + el.textContent.trim().slice(0, 24));
  }
  return out;
}"""

RUNNING_JS = """() => document.getAnimations()
  .filter(a => a.playState === 'running')
  .map(a => (a.animationName || a.transitionProperty || 'waapi') + '@' + (a.effect?.target?.id || a.effect?.target?.className || '?'))"""


def check(cond, msg, fails):
    if not cond:
        fails.append(msg)


def scrollbar(browser, fails):
    for theme in ("light", "dark"):
        ctx = browser.new_context(viewport={"width": 1280, "height": 800})
        page = ctx.new_page()
        page.goto(URL + f"?page=overview&theme={theme}")
        page.wait_for_timeout(700)
        main = page.locator("#main")
        w0 = main.evaluate("e => e.clientWidth")
        # 滚动出现后内容宽度不变（不占布局）
        main.evaluate("e => e.scrollTo({top: 200, behavior: 'instant'})")
        page.wait_for_timeout(200)
        check(main.evaluate("e => e.clientWidth") == w0, f"[{theme}] 滚动条出现时 main 宽度变化", fails)
        check(page.evaluate("document.documentElement.scrollWidth <= innerWidth"), f"[{theme}] 横向溢出", fails)
        sb = page.locator(".sb.on").first
        box = sb.bounding_box()
        thumb = page.locator(".sb-thumb").first
        # 淡出：1 秒内仍在，约 1.2 秒后消失
        page.wait_for_timeout(700)
        check(page.locator(".sb.on").count() == 1, f"[{theme}] 停止 0.9 秒时滚动条已提前隐藏", fails)
        page.wait_for_timeout(600)
        check(page.locator(".sb.on").count() == 0, f"[{theme}] 停止 1.5 秒后滚动条未隐藏", fails)
        # 点击轨道底部跳转
        main.evaluate("e => e.scrollTo({top: 0, behavior: 'instant'})")
        page.wait_for_timeout(150)
        page.mouse.move(box["x"] + box["width"] / 2, box["y"] + box["height"] - 6)
        page.wait_for_timeout(300)
        track_bg = page.locator(".sb").first.evaluate("e => getComputedStyle(e).backgroundColor")
        check(track_bg not in ("rgba(0, 0, 0, 0)", "transparent"), f"[{theme}] 悬停时无轨道背景", fails)
        page.mouse.down(); page.mouse.up()
        page.wait_for_timeout(150)
        st, mx = main.evaluate("e => [e.scrollTop, e.scrollHeight - e.clientHeight]")
        check(st > mx * 0.8, f"[{theme}] 点击轨道底部未跳到底部 ({st}/{mx})", fails)
        # 拖动滑块回到顶部
        tb = thumb.bounding_box()
        page.mouse.move(tb["x"] + tb["width"] / 2, tb["y"] + tb["height"] / 2)
        page.mouse.down()
        page.mouse.move(tb["x"] + tb["width"] / 2, box["y"] - 200, steps=6)
        check(page.locator(".sb.drag").count() == 1, f"[{theme}] 拖动时无 drag 状态", fails)
        check(thumb.evaluate("e => e.offsetWidth") == 10, f"[{theme}] 拖动时滑块未加宽", fails)
        page.mouse.up()
        check(main.evaluate("e => e.scrollTop") < 5, f"[{theme}] 拖动滑块未回到顶部", fails)
        # 在滚动条上滚轮
        page.mouse.move(box["x"] + box["width"] / 2, box["y"] + 60)
        page.mouse.wheel(0, 300)
        page.wait_for_timeout(200)
        check(main.evaluate("e => e.scrollTop") > 100, f"[{theme}] 滚动条上滚轮无效", fails)
        # 滑块与背景对比：滑块颜色 alpha 足够
        col = thumb.evaluate("e => getComputedStyle(e).backgroundColor")
        alpha = float(col.rsplit(",", 1)[1].strip(" )")) if col.startswith("rgba") else 1
        check(alpha >= 0.3, f"[{theme}] 滑块对比度不足 {col}", fails)
        ctx.close()


def sidebar_transition(browser, fails):
    ctx = browser.new_context(viewport={"width": 1280, "height": 800})
    page = ctx.new_page()
    page.goto(URL + "?page=overview")
    page.wait_for_timeout(600)
    one_line = "() => [...document.querySelectorAll('#nav button')].every(b => b.offsetHeight <= 37 && b.scrollHeight <= 37)"
    page.click("#side-toggle")
    for t in (60, 140, 260):
        page.wait_for_timeout(t if t == 60 else t - 60 if t == 140 else 120)
        check(page.evaluate(one_line), f"侧栏收起过渡 {t}ms 时导航文字换行/挤压", fails)
    page.wait_for_timeout(600)
    check(page.evaluate(one_line), "侧栏收起后导航文字换行", fails)
    page.click("#side-toggle")
    page.wait_for_timeout(100)
    check(page.evaluate(one_line), "侧栏展开过渡中导航文字换行/挤压", fails)
    ctx.close()


def sync_conflict(browser, fails):
    ctx = browser.new_context(viewport={"width": 1280, "height": 800})
    page = ctx.new_page()
    page.goto(URL + "?page=fan")
    page.wait_for_timeout(600)
    # 保留草稿：冲突收起，标签仍为待应用，读回为设备值
    page.click(".seg[data-seg=profile] [data-v=turbo]")
    page.click("#pv-fab"); page.click("#pv-ext"); page.click("#pv-fab")
    page.wait_for_timeout(150)
    # 同步中重复点击只产生一次同步
    n0 = page.locator(".toast").count()
    for _ in range(4):
        page.click("#refresh-btn")
    page.wait_for_timeout(2200)
    check(page.locator(".toast").count() - n0 <= 1, "同步中重复点击产生多次同步提示", fails)
    expect(page.locator("#profile-cf")).to_have_class("collapse cf open")
    page.click("[data-keep=profile]")
    page.wait_for_timeout(500)
    check("open" not in page.locator("#profile-cf").get_attribute("class"), "保留草稿后冲突未收起", fails)
    check(page.locator("#profile-tag").inner_text().startswith("待应用 · 极速"), "保留草稿后标签不是待应用", fails)
    cur = page.locator(".seg[data-seg=profile] [data-current]").get_attribute("data-v")
    check(cur not in (None, "turbo"), "保留草稿后读回标记丢失", fails)
    check(page.locator(".seg[data-seg=profile]").get_attribute("data-value") == "turbo", "保留草稿后草稿被覆盖", fails)
    # 同步结束后刷新按钮恢复
    check(not page.locator("#refresh-btn").evaluate("e => e.classList.contains('syncing')"), "同步结束后按钮仍在转圈", fails)

    # 外设：保留草稿
    page.goto(URL + "?page=dc")
    page.wait_for_timeout(600)
    page.click(".seg[data-seg=dc-mouse-dpi] [data-v='800']")
    page.click("#pv-fab"); page.click("#pv-ext"); page.click("#pv-fab")
    expect(page.locator("#dc-cf-mouse")).to_have_class("collapse cf open", timeout=3000)
    page.click("[data-keep=mouse]")
    page.wait_for_timeout(500)
    check("open" not in page.locator("#dc-cf-mouse").get_attribute("class"), "外设保留草稿后冲突未收起", fails)
    check(page.locator("#dc-mouse-tag").inner_text().startswith("待应用"), "外设保留草稿后标签不是待应用", fails)
    check(page.locator(".seg[data-seg=dc-mouse-dpi]").get_attribute("data-value") == "800", "外设草稿被覆盖", fails)
    ctx.close()


def dc_extras(browser, fails):
    ctx = browser.new_context(viewport={"width": 1280, "height": 800})
    page = ctx.new_page()
    page.goto(URL + "?page=dc")
    page.wait_for_timeout(600)
    disc = page.locator("[aria-controls=dc-soon]")
    disc.click()
    page.wait_for_timeout(500)
    check(disc.get_attribute("aria-expanded") == "true", "二期折叠未展开", fails)
    check("open" in (page.locator("#dc-soon").get_attribute("class") or ""), "二期内容未显示", fails)
    disc.click()
    page.wait_for_timeout(400)
    check(disc.get_attribute("aria-expanded") == "false", "二期折叠未收起", fails)
    # 文件变更：待确认、设备未知、确认后运行
    page.click("#pv-fab"); page.click("#pv-dchash"); page.click("#pv-fab")
    page.wait_for_timeout(500)
    check(page.locator("#dc-mod-tag").inner_text() == "待确认", "哈希变更后模块标签不是待确认", fails)
    check("open" in page.locator("#dc-hash").get_attribute("class"), "哈希变更提示未展开", fails)
    check(page.locator("#dc-proc").inner_text() == "未运行", "哈希变更后进程仍显示运行", fails)
    check(page.locator("#dc-mouse-tag").inner_text() == "状态未知", "模块未运行时鼠标仍显示读数", fails)
    check(page.locator("[data-roll='dcBatt-mouse']").get_attribute("aria-label") == "—", "模块未运行时电量不是 —", fails)
    page.locator("#main").evaluate("e => e.scrollTo({top: 0, behavior: 'instant'})")
    page.wait_for_timeout(2600)  # 等提示条退出
    page.screenshot(path=str(SHOTS / "feature-dc-hash.png"))
    page.click("#dc-trust")
    page.wait_for_timeout(1500)
    check(page.locator("#dc-mod-tag").inner_text() == "运行中", "确认后模块未运行", fails)
    check("open" not in page.locator("#dc-hash").get_attribute("class"), "确认后哈希提示未收起", fails)
    ctx.close()


def matrix(browser, fails):
    for w in (1280, 1024):
        for theme in ("light", "dark"):
            ctx = browser.new_context(viewport={"width": w, "height": 800})
            page = ctx.new_page()
            errs = []
            page.on("pageerror", lambda e: errs.append(str(e)))
            page.on("console", lambda m: m.type == "error" and errs.append(m.text))
            for pg, extra in PAGES:
                tag = f"[{w} {theme} {pg}{extra}]"
                page.goto(URL + f"?page={pg}{extra}&theme={theme}")
                page.wait_for_timeout(1800)
                check(page.evaluate("document.documentElement.scrollWidth <= innerWidth"), f"{tag} 横向溢出", fails)
                check(page.locator("#main").evaluate("e => e.scrollWidth <= e.clientWidth"), f"{tag} main 横向溢出", fails)
                clipped = page.evaluate(CLIPPED_JS)
                check(not clipped, f"{tag} 截字: {clipped[:5]}", fails)
                running = page.evaluate(RUNNING_JS)
                check(not running, f"{tag} 静止时仍有动画: {running[:5]}", fails)
            check(not errs, f"[{w} {theme}] 控制台错误: {errs[:3]}", fails)
            ctx.close()


def reduced_motion(browser, fails):
    ctx = browser.new_context(viewport={"width": 1280, "height": 800}, reduced_motion="reduce")
    page = ctx.new_page()
    page.goto(URL + "?page=overview")
    page.wait_for_timeout(500)
    check(page.evaluate("document.documentElement.classList.contains('rm')"), "系统减少动效未生效", fails)
    page.click("#nav button[data-page=fan]")
    page.wait_for_timeout(30)
    durs = page.evaluate("document.getAnimations().map(a => a.effect.getComputedTiming().duration).filter(d => d > 0)")
    check(all(d <= 141 for d in durs), f"减少动效下有超过 140ms 的动画: {durs[:6]}", fails)
    page.click("#refresh-btn")
    page.wait_for_timeout(100)
    spin = page.locator("#refresh-ico").evaluate("e => getComputedStyle(e).animationName")
    check(spin == "none", f"减少动效下刷新图标仍旋转: {spin}", fails)
    page.wait_for_timeout(1600)
    ctx.close()


def main() -> int:
    fails: list[str] = []
    with sync_playwright() as p:
        browser = p.chromium.launch(channel="msedge")
        for fn in (scrollbar, sidebar_transition, sync_conflict, dc_extras, matrix, reduced_motion):
            try:
                fn(browser, fails)
            except Exception as e:  # noqa: BLE001
                fails.append(f"{fn.__name__} 异常: {e}")
            print(f"{fn.__name__}: 完成")
        browser.close()
    for f in fails:
        print("FAIL", f)
    print("PASS" if not fails else f"{len(fails)} 项失败")
    return 1 if fails else 0


if __name__ == "__main__":
    sys.exit(main())
