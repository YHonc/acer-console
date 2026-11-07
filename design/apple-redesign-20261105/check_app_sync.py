"""U2: real built frontend, Edge, entirely in-memory transport; never calls hardware.

Run after vite preview on 4173. Also visits the plain browser without the fixture.
"""
import pathlib
import sys
from playwright.sync_api import sync_playwright, expect

HERE = pathlib.Path(__file__).parent
BASE = sys.argv[1] if len(sys.argv) > 1 else 'http://127.0.0.1:4173/'
NAV = [('设备总览', None), ('性能与风扇', None), ('设备设置', '键盘灯光'),
       ('设备设置', '硬件辅助与电源'), ('设备设置', '应用与系统'),
       ('服务插件', None)]  # DC has its own transport/read coalescing coverage in check_app_dc.py.


def go(page, nav, sub=None):
    page.get_by_role('button', name=nav, exact=True).first.click()
    page.wait_for_timeout(220)
    if sub:
        page.get_by_role('radio', name=sub, exact=True).click()
    page.wait_for_timeout(200)


def controls(page):
    return page.locator('main').evaluate('''root => [...root.querySelectorAll('button,input,select')]
      .filter(e => e.getClientRects().length && !e.closest('[inert]'))
      .map((e, index) => [e.getAttribute('aria-label') || e.id || `${index}:${e.tagName}:${e.getAttribute('type')}`, e.matches(':disabled')])''')


def count(page, command):
    return page.evaluate('(command) => syncFixture.calls.filter(c => c.command === command).length', command)


def release(page, command):
    page.evaluate('''command => { syncFixture.holds[command] = false;
      (syncFixture.releases[command] || []).splice(0).forEach(resolve => resolve()); }''', command)


def main():
    with sync_playwright() as p:
        browser = p.chromium.launch(channel='msedge')
        plain = browser.new_page()
        errors = []
        plain.on('pageerror', lambda error: errors.append(str(error)))
        plain.goto(BASE)
        plain.get_by_role('button', name='刷新设备状态').click()
        expect(plain.get_by_role('button', name='刷新设备状态')).to_be_enabled()
        plain.close()

        ctx = browser.new_context(viewport={'width': 1280, 'height': 800}, reduced_motion='reduce')
        ctx.add_init_script(path=str(HERE / 'check_app_sync_fixture.js'))
        page = ctx.new_page()
        page.on('pageerror', lambda error: errors.append(str(error)))
        page.goto(BASE)
        page.wait_for_timeout(400)
        for nav, sub in NAV:
            go(page, nav, sub)
            before = controls(page)
            page.evaluate('syncFixture.delay = 1000')
            n = count(page, 'get_snapshot')
            refresh = page.get_by_role('button', name='刷新设备状态')
            refresh.click()
            expect(refresh).to_have_attribute('aria-busy', 'true')
            expect(refresh).to_be_enabled()
            refresh.click()
            refresh.click()
            assert count(page, 'get_snapshot') == n + 1, 'Repeated refresh did not coalesce'
            assert before == controls(page), f'Disabled changed during refresh: {nav}/{sub}'
            expect(refresh).to_have_attribute('aria-busy', 'false')
            page.evaluate('syncFixture.delay = 0')
            print('OK sync controls:', nav, sub or '')

        # An edited profile survives external readback; both conflict choices work.
        go(page, '性能与风扇')
        profile = page.get_by_role('radiogroup', name='性能模式草稿')
        profile.get_by_role('radio', name='安静', exact=True).click()
        page.evaluate("syncFixture.snapshot.profile = 'performance'")
        page.get_by_role('button', name='刷新设备状态').click()
        conflict = page.locator('.conflict').filter(has_text='设备已变为 性能')
        expect(conflict).to_be_visible()
        (HERE.parent.parent / 'work' / 'U2').mkdir(parents=True, exist_ok=True)
        page.screenshot(path=str(HERE.parent.parent / 'work' / 'U2' / 'profile-conflict.png'))
        conflict.get_by_role('button', name='保留草稿').click()
        expect(conflict.locator('..')).to_have_attribute('inert', '')
        expect(profile.get_by_role('radio', name='安静', exact=True)).to_have_attribute('aria-checked', 'true')
        page.evaluate("syncFixture.snapshot.profile = 'turbo'")
        page.get_by_role('button', name='刷新设备状态').click()
        conflict = page.locator('.conflict').filter(has_text='设备已变为 极速')
        expect(conflict).to_be_visible()
        conflict.get_by_role('button', name='采用设备值').click()
        expect(profile.get_by_role('radio', name='极速', exact=True)).to_have_attribute('aria-checked', 'true')
        profile.get_by_role('radio', name='安静', exact=True).click()
        page.evaluate("syncFixture.snapshot.profile = 'balanced'")
        page.get_by_role('button', name='刷新设备状态').click()
        conflict = page.locator('.conflict').filter(has_text='设备已变为 平衡')
        expect(conflict.locator('..')).not_to_have_attribute('inert', '')
        profile.get_by_role('radio', name='平衡', exact=True).click()
        expect(conflict.locator('..')).to_have_attribute('inert', '')
        profile.get_by_role('radio', name='安静', exact=True).click()
        page.evaluate("syncFixture.snapshot.profile = 'turbo'")
        page.get_by_role('button', name='刷新设备状态').click()
        conflict = page.locator('.conflict').filter(has_text='设备已变为 极速')
        expect(conflict.locator('..')).not_to_have_attribute('inert', '')
        page.get_by_role('button', name='应用模式', exact=True).click()
        expect(page.locator('.conflict').first.locator('..')).to_have_attribute('inert', '')
        print('OK profile conflict keep/adopt/auto-close on matching edit and apply')

        # A performance write locks lighting via the controller, leaving settings editable.
        profile.get_by_role('radio', name='平衡', exact=True).click()
        page.evaluate("syncFixture.holds.set_profile = true")
        page.get_by_role('button', name='应用模式', exact=True).click()
        go(page, '设备设置', '键盘灯光')
        expect(page.get_by_role('button', name='呼吸', exact=True)).to_be_enabled()
        expect(page.get_by_role('button', name='应用静态灯光', exact=True)).to_be_disabled()
        refresh = page.get_by_role('button', name='刷新设备状态')
        refresh.click()
        expect(refresh).to_be_enabled()
        go(page, '设备设置', '应用与系统')
        expect(page.get_by_role('button', name='采样更新频率')).to_be_enabled()
        release(page, 'set_profile')
        page.wait_for_timeout(100)
        print('OK write resource cross-lock and unrelated editing')

        # Saving keeps a new theme draft made after request dispatch.
        page.get_by_role('button', name='采样更新频率').click()
        page.get_by_role('listbox', name='采样更新频率').press('End')
        page.get_by_role('listbox', name='采样更新频率').press('Enter')
        page.evaluate('syncFixture.holds.save_settings = true')
        page.get_by_role('button', name='保存设置', exact=True).click()
        page.get_by_role('radiogroup', name='主题外观').get_by_role('radio', name='深色', exact=True).click()
        release(page, 'save_settings')
        expect(page.get_by_role('radiogroup', name='主题外观').get_by_role('radio', name='深色', exact=True)).to_have_attribute('aria-checked', 'true')
        expect(page.get_by_role('button', name='保存设置', exact=True)).to_be_enabled()
        print('OK settings edits survive save')

        go(page, '设备设置', '硬件辅助与电源')
        audio = page.get_by_role('group', name='音效预设', exact=True)
        usb = page.get_by_role('group', name='USB关机供电', exact=True)
        vendor = page.get_by_role('group', name='厂商设备设置', exact=True)
        for group, command in ((audio, 'get_audio'), (usb, 'get_usb_charge'), (vendor, 'get_vendor_controls')):
            page.evaluate('syncFixture.delay = 800')
            group.get_by_role('button', name='刷新状态', exact=True).click()
            expect(group.get_by_role('button', name='读取中…', exact=True)).to_be_enabled()
            editable = group.locator('select,input[type=checkbox],input[type=radio]')
            assert all(not editable.nth(i).is_disabled() for i in range(editable.count())), command
            page.wait_for_timeout(850)
        page.evaluate('syncFixture.delay = 0')
        # Concurrent vendor switches have independent locks and readback merges.
        page.evaluate('syncFixture.holds.set_vendor_control = true')
        vendor.get_by_role('switch', name='LCD响应加速', exact=True).click()
        expect(vendor.get_by_role('switch', name='启动声音')).to_be_enabled()
        vendor.get_by_role('switch', name='启动声音').click()
        expect(vendor.get_by_role('switch', name='LCD响应加速', exact=True)).to_be_disabled()
        expect(vendor.get_by_role('switch', name='粘滞键')).to_be_enabled()
        release(page, 'set_vendor_control')
        expect(vendor.get_by_role('switch', name='LCD响应加速', exact=True)).to_be_checked()
        expect(vendor.get_by_role('switch', name='启动声音')).to_be_checked()
        page.evaluate("syncFixture.fail = ['get_audio', 'get_usb_charge', 'get_vendor_controls']")
        for group in (audio, usb, vendor):
            group.get_by_role('button', name='刷新状态', exact=True).click()
        expect(audio.get_by_role('button', name='DTS 音效')).to_be_disabled()
        expect(usb.get_by_role('switch')).to_be_disabled()
        expect(vendor.get_by_role('switch', name='粘滞键')).to_be_disabled()
        print('OK last-confirmed editing, vendor single-item locks, failed reads stale')

        # A newer post-write snapshot wins even if an older manual refresh finishes last.
        page.evaluate('syncFixture.fail = []; syncFixture.settings.poll_interval_ms = 60000')
        go(page, '性能与风扇')
        page.evaluate('syncFixture.holds.get_snapshot = true')
        page.get_by_role('button', name='刷新设备状态').click()
        profile = page.get_by_role('radiogroup', name='性能模式草稿')
        profile.get_by_role('radio', name='性能', exact=True).click()
        page.get_by_role('button', name='应用模式', exact=True).click()
        page.wait_for_function('syncFixture.releases.get_snapshot?.length === 2')
        page.evaluate('syncFixture.releases.get_snapshot.pop()()')
        expect(profile.get_by_role('radio', name='性能', exact=True)).to_have_attribute('data-current', '')
        release(page, 'get_snapshot')
        expect(page.get_by_role('button', name='刷新设备状态')).to_have_attribute('aria-busy', 'false')
        expect(profile.get_by_role('radio', name='性能', exact=True)).to_have_attribute('data-current', '')
        print('OK old snapshot cannot overwrite newer readback')

        go(page, '设备设置', '键盘灯光')
        page.get_by_role('spinbutton', name='灯光亮度百分比数值').fill('55')
        page.evaluate("syncFixture.snapshot.lighting_config.effect = 'breathing'; syncFixture.snapshot.lighting_config.brightness = 70")
        page.get_by_role('button', name='刷新设备状态').click()
        expect(page.get_by_role('button', name='常亮 · 四区静态', exact=True)).to_have_attribute('aria-pressed', 'true')
        conflict = page.locator('.conflict').filter(has_text='设备已变为 呼吸')
        expect(conflict.locator('..')).not_to_have_attribute('inert', '')
        conflict.get_by_role('button', name='采用设备值').click()
        expect(page.locator('button.fx').filter(has_text='呼吸')).to_have_attribute('aria-pressed', 'true')
        print('OK static lighting draft survives external effect change')
        assert not errors, errors
        print('OK no JS errors; browser-only transport, no hardware')
        ctx.close()
        browser.close()
    return 0


if __name__ == '__main__':
    sys.exit(main())
