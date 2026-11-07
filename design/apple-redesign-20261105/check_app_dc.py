"""U3: production Svelte UI, browser-only DC transport, Edge. No device process."""
from pathlib import Path
import sys
from playwright.sync_api import sync_playwright, expect

HERE=Path(__file__).parent
BASE=sys.argv[1] if len(sys.argv)>1 else 'http://127.0.0.1:4173/'
OUT=HERE/'shots-app'
STATES={'starting':('启动中','正在启动模块'),'reconnecting':('重连中','正在重连（第 2/3 次）'),'stopped':('已停止','模块未运行'),'elsewhere':('被占用','已在其他地方运行'),'incompatible':('不兼容','协议版本不兼容'),'error':('异常','模块异常退出'),'untrusted':('待确认','模块文件已变更，确认后启动')}

def go(page):
    page.get_by_role('button',name='Device Center 外设',exact=True).click()
    expect(page.locator('#dc-mod-tag')).to_be_visible()
    expect(page.locator('#dc-devs .dev-row')).to_have_count(3)

def order(page):
    return page.locator('#dc-devs .dev-row').evaluate_all("rows=>rows.map(r=>r.dataset.id)")

def main():
    OUT.mkdir(exist_ok=True)
    errors=[]
    with sync_playwright() as p:
        browser=p.chromium.launch(channel='msedge')
        for theme in ('light','dark'):
            ctx=browser.new_context(viewport={'width':1280,'height':800})
            ctx.add_init_script(path=str(HERE/'check_app_sync_fixture.js'))
            ctx.add_init_script(f"window.syncFixture.settings.theme='{theme}';")
            page=ctx.new_page()
            page.on('pageerror',lambda e:errors.append(str(e)))
            page.goto(BASE)
            page.evaluate("theme=>{document.documentElement.dataset.theme=theme;syncFixture.settings.theme=theme}",theme)
            go(page)
            assert page.evaluate('document.documentElement.dataset.theme')==theme
            expect(page.locator('[data-g=mouse] .strong')).to_have_text('1600')
            page.wait_for_timeout(750)
            page.screenshot(path=str(OUT/f'dc-{theme}-running.png'))
            page.set_viewport_size({'width':1024,'height':768})
            page.wait_for_timeout(550);page.screenshot(path=str(OUT/f'dc-{theme}-1024.png'))
            page.set_viewport_size({'width':1280,'height':800})
            # All eight visible states, clear readings and block writes; module controls stay available.
            for state,(tag,note) in STATES.items():
                page.evaluate('s=>syncFixture.setDcState(s)',state)
                expect(page.locator('#dc-mod-tag')).to_have_text(tag)
                expect(page.locator('#dc-hash' if state=='untrusted' else '#dc-note')).to_contain_text(note)
                expect(page.locator('[data-g=mouse] .strong')).to_have_text('—')
                expect(page.locator('[data-g=mouse]').get_by_role('button',name='应用',exact=True)).to_be_disabled()
                assert page.locator('#dc-devs').inner_text().count('状态未知')==3
                assert page.evaluate('document.documentElement.scrollWidth<=innerWidth')
                page.wait_for_timeout(550)
                if state in ('reconnecting','error','untrusted'):
                    page.screenshot(path=str(OUT/f'dc-{theme}-{state}.png'))
            page.evaluate("syncFixture.setDcState('stopped')")
            page.get_by_role('button',name='启动模块',exact=True).click()
            expect(page.locator('[data-g=mouse] .strong')).to_have_text('1600')
            # Resource reads coalesce without locking drafts.
            page.evaluate('syncFixture.delay=250')
            refresh=page.get_by_role('button',name='刷新设备状态')
            n=page.evaluate("syncFixture.calls.filter(c=>c.command==='dc_call'&&c.args.method==='device.list').length")
            refresh.click();refresh.click();refresh.click()
            expect(page.get_by_role('radiogroup',name='键盘回报率',exact=True).get_by_role('radio',name='125',exact=True)).to_be_enabled()
            expect(page.get_by_role('button',name='休眠时间',exact=True)).to_be_enabled()
            page.wait_for_timeout(2200)
            assert page.evaluate("syncFixture.calls.filter(c=>c.command==='dc_call'&&c.args.method==='device.list').length")==n+1
            page.evaluate('syncFixture.delay=0')
            # Host recovery events update only their device/group, including failure -> unknown.
            expect(page.locator('[data-id=mouse] .row-v')).to_have_text('电量 72%')
            page.evaluate("syncFixture.readback('mouse.dpi.read','mouse',{...syncFixture.dcData.dpi,levels:syncFixture.dcData.dpi.levels.map(l=>({...l,xDpi:l.level===2?1800:l.xDpi}))})")
            expect(page.locator('[data-g=mouse] .strong')).to_have_text('1800')
            page.evaluate("syncFixture.readback('mouse.dpi.read','mouse',null,{code:'TIMEOUT',message:'read failed',retryable:true,details:null})")
            expect(page.locator('[data-g=mouse] .strong')).to_have_text('—')
            page.evaluate("syncFixture.readback('battery.read','mouse',null,{code:'TIMEOUT'})")
            expect(page.locator('[data-id=mouse] .row-v')).to_have_text('电量 —')
            refresh.click();expect(page.locator('[data-g=mouse] .strong')).to_have_text('1600')
            rate=page.get_by_role('radiogroup',name='键盘回报率',exact=True)
            rate.get_by_role('radio',name='125',exact=True).click()
            page.evaluate("syncFixture.readback('settings.read','mouse',{...syncFixture.dcData.settings.mouse,fields:syncFixture.dcData.settings.mouse.fields.map(f=>({...f,value:f.key==='reportRateHz'?500:f.value}))})")
            expect(page.locator('[data-g=kb] .collapse.open')).to_have_count(0)
            page.evaluate("syncFixture.readback('settings.read','kb',{...syncFixture.dcData.settings.kb,fields:syncFixture.dcData.settings.kb.fields.map(f=>({...f,value:f.key==='reportRateHz'?500:f.value}))})")
            expect(rate.get_by_role('radio',name='125',exact=True)).to_have_attribute('aria-checked','true')
            expect(page.locator('[data-g=kb] .conflict')).to_contain_text('设备已变为')
            page.wait_for_timeout(550);page.screenshot(path=str(OUT/f'dc-{theme}-readback-conflict.png'))
            page.locator('[data-g=kb] .conflict').get_by_role('button',name='采用设备值').click()
            refresh.click();expect(rate.get_by_role('radio',name='1000 Hz',exact=True)).to_have_attribute('aria-checked','true')
            # Popup keyboard, readback dot, alignment, initials and dismissal.
            sleep=page.get_by_role('button',name='休眠时间',exact=True)
            sleep.scroll_into_view_if_needed();page.wait_for_timeout(500);sleep.press('ArrowDown')
            menu=page.get_by_role('listbox',name='休眠时间',exact=True)
            expect(menu).to_be_visible();expect(menu.locator('.pop-cur')).to_have_count(1)
            assert page.locator('select:visible').count()==0
            assert menu.evaluate("e=>getComputedStyle(e).backdropFilter")!='none'
            if menu.get_attribute('data-place')=='align':
                selected=menu.locator('[aria-selected=true]').bounding_box();button=sleep.bounding_box()
                assert abs(selected['y']+selected['height']/2-button['y']-button['height']/2)<2
            page.wait_for_timeout(450);page.screenshot(path=str(OUT/f'dc-{theme}-popup.png'))
            menu.press('c');menu.press('Enter');expect(sleep.locator('.on')).to_have_text('从不')
            sleep.press('Enter');page.get_by_role('listbox',name='休眠时间').press('Home');page.get_by_role('listbox',name='休眠时间').press('Enter')
            expect(sleep.locator('.on')).to_have_text('1 分钟')
            sleep.press('Enter');page.get_by_role('listbox',name='休眠时间').press('End');page.get_by_role('listbox',name='休眠时间').press('Escape')
            expect(sleep).to_be_focused()
            sleep.press('Enter');page.get_by_role('listbox',name='休眠时间').press('Tab');expect(sleep).to_be_focused()
            sleep.press('Enter');page.mouse.click(300,100);expect(page.get_by_role('listbox')).to_have_count(0)
            sleep.press('Enter');page.locator('main').evaluate('e=>e.scrollBy(0,80)');expect(page.get_by_role('listbox')).to_have_count(0)
            # Readback conflict retains draft and both resolution buttons work.
            page.evaluate("syncFixture.dcData.settings.kb.fields.find(f=>f.key==='sleepSeconds').value=600")
            refresh.click()
            conflict=page.locator('[data-g=kb] .conflict')
            expect(conflict).to_contain_text('设备已变为')
            page.wait_for_timeout(700);page.screenshot(path=str(OUT/f'dc-{theme}-conflict.png'))
            conflict.get_by_role('button',name='保留草稿').click()
            expect(sleep.locator('.on')).to_have_text('1 分钟')
            page.evaluate("syncFixture.dcData.settings.kb.fields.find(f=>f.key==='sleepSeconds').value=1800")
            refresh.click();conflict.get_by_role('button',name='采用设备值').click();expect(sleep.locator('.on')).to_have_text('30 分钟')
            # Immediate switch only locks itself. A timeout is unknown; no frontend recovery duplicate.
            page.evaluate("syncFixture.holds['settings.apply']=true")
            low=page.get_by_role('switch',name='低延迟模式',exact=True)
            low.click();expect(low).to_be_disabled();expect(page.get_by_role('switch',name='Mac 模式',exact=True)).to_be_enabled()
            page.evaluate("syncFixture.dcFailure={code:'TIMEOUT',message:'disconnected',details:{reason:'disconnected'},retryable:true};syncFixture.holds['settings.apply']=false;syncFixture.releases['settings.apply'].splice(0).forEach(r=>r())")
            expect(page.get_by_text('结果未知，请刷新读回',exact=True)).to_be_visible()
            expect(low).to_be_checked()  # A failed write must return to the last confirmed readback.
            page.evaluate('syncFixture.dcFailure=null')
            page.evaluate("syncFixture.readback('settings.read','mouse',syncFixture.dcData.settings.mouse)")
            expect(page.get_by_text('结果未知，请刷新读回',exact=True)).to_be_visible()
            page.evaluate("syncFixture.readback('settings.read','kb',syncFixture.dcData.settings.kb)")
            expect(page.get_by_text('结果未知，请刷新读回',exact=True)).to_have_count(0)
            # PARTIAL_WRITE recovery can also fail; old readback must disappear.
            page.evaluate("syncFixture.dcFailure={code:'PARTIAL_WRITE',message:'partial',retryable:true,details:null}")
            low.click();expect(page.get_by_text('结果未知，请刷新读回',exact=True)).to_be_visible()
            page.evaluate("syncFixture.dcFailure=null;syncFixture.readback('settings.read','kb',null,{code:'TIMEOUT'})")
            expect(page.get_by_text('结果未知，请刷新读回',exact=True)).to_have_count(0)
            expect(page.get_by_role('switch',name='低延迟模式',exact=True)).to_have_count(0)
            expect(page.get_by_role('button',name='休眠时间',exact=True).locator('.on')).to_have_text('—')
            page.wait_for_timeout(550);page.screenshot(path=str(OUT/f'dc-{theme}-readback-failed.png'))
            refresh.click();expect(low).to_be_visible()
            page.evaluate("syncFixture.dcFailure={code:'PARTIAL_WRITE',message:'partial',retryable:true,details:null};syncFixture.dcRecoveryBeforeReturn={method:'settings.read',deviceId:'kb',result:syncFixture.dcData.settings.kb}")
            low.click()
            expect(low).to_be_enabled()
            expect(page.get_by_text('结果未知，请刷新读回',exact=True)).to_have_count(0)
            page.evaluate("syncFixture.dcFailure=null;syncFixture.readback('keyboard.lighting.read','kb',{...syncFixture.dcData.lighting,brightness:2})")
            expect(page.locator('[data-g=kbl] input[type=range]').first).to_have_value('2')
            page.evaluate("syncFixture.readback('keyboard.lighting.read','kb',null,{code:'TIMEOUT'})")
            expect(page.locator('[data-g=kbl] output').first).to_have_text('—')
            page.evaluate("syncFixture.readback('lamp.read',null,{...syncFixture.dcData.lamp,brightness:45})")
            expect(page.locator('[data-g=lamp] output').first).to_have_text('45%')
            page.evaluate("syncFixture.readback('lamp.read',null,null,{code:'TIMEOUT'})")
            expect(page.locator('[data-g=lamp] output').first).to_have_text('—')
            page.evaluate("syncFixture.readback('device.snapshots',null,{devices:[{deviceId:'mouse',batteryPercent:54,connection:'connected'}],lamp:{...syncFixture.dcData.lamp,brightness:35,connection:'connected'}})")
            expect(page.locator('[data-id=mouse] .row-v')).to_have_text('电量 54%')
            expect(page.locator('[data-g=lamp] output').first).to_have_text('35%')
            page.evaluate("syncFixture.readback('device.snapshots',null,null,{code:'TIMEOUT'})")
            expect(page.locator('[data-id=mouse] .row-v')).to_have_text('电量 —')
            expect(page.locator('[data-g=mouse] .strong')).to_have_text('—')
            expect(page.locator('[data-g=lamp] output').first).to_have_text('—')
            refresh.click()
            # Device fields remain independent: lamp brightness must not enter keyboard drafts.
            expect(page.locator('[data-g=kbl] input[type=range]').first).to_have_value('3')
            mode=page.get_by_role('button',name='键盘灯效模式')
            mode.scroll_into_view_if_needed();page.wait_for_timeout(450)
            page.screenshot(path=str(OUT/f'dc-{theme}-lighting.png'))
            # DPI selection switches the active level, not the values stored in a level.
            page.get_by_role('radiogroup',name='DPI 档位').get_by_role('radio',name='3200',exact=True).click()
            page.locator('[data-g=mouse]').get_by_role('button',name='应用',exact=True).click()
            expect(page.locator('[data-g=mouse] .strong')).to_have_text('3200')
            assert page.evaluate("syncFixture.calls.filter(c=>c.args?.method==='mouse.dpi.apply').length")==0
            # Keyboard order, settings order, persisted order, disconnected placement, append.
            grip=page.get_by_role('button',name='调整顺序：KX75 键盘')
            grip.press('Home');assert order(page)[0]=='kb'
            expect(page.locator('#dc-live')).to_contain_text('移到第 1 位')
            assert page.locator('#dc-secs .dc-sec').first.get_attribute('data-sec')=='kb'
            page.wait_for_timeout(450);page.screenshot(path=str(OUT/f'dc-{theme}-order.png'))
            page.reload();go(page);assert order(page)[0]=='kb'
            assert page.evaluate('document.documentElement.dataset.theme')==theme
            # Pointer row follows the grip; other rows make space; release persists order.
            mouse_grip=page.get_by_role('button',name='调整顺序：ATK F1 鼠标')
            mouse_grip.scroll_into_view_if_needed();box=mouse_grip.bounding_box();first=page.locator('#dc-devs .dev-row').first.bounding_box()
            page.mouse.move(box['x']+10,box['y']+12);page.mouse.down();page.mouse.move(box['x']+10,first['y']+10,steps=12)
            expect(page.locator('.dev-row.dragging')).to_have_count(1)
            page.mouse.up();page.wait_for_timeout(450);assert order(page)[0]=='mouse'
            page.get_by_role('button',name='调整顺序：KX75 键盘').press('Home')
            page.evaluate("syncFixture.dcData.disconnected.push('kb');syncFixture.emit('dc://event',{type:'event',seq:1,name:'device.changed',deviceId:'kb',data:{connected:false}})")
            expect(page.locator('#dc-devs .dev-row').first).to_contain_text('已断开');assert order(page)[0]=='kb'
            page.evaluate("syncFixture.dcData.hid.push({...syncFixture.dcData.hid[0],id:'new',product:'新设备'});syncFixture.emit('dc://event',{type:'event',seq:3,name:'device.changed',deviceId:'new',data:{connected:true}})")
            expect(page.locator('#dc-devs .dev-row')).to_have_count(4);assert order(page)[-1]=='new'
            # Phase two contains labels only; token never appears in response or DOM.
            page.get_by_role('button',name='即将支持（二期）').click()
            assert page.locator('#dc-soon button,#dc-soon input').count()==0
            assert '7c2e' in page.locator('main').inner_text()
            page.get_by_role('button',name='更换',exact=True).click()
            token=page.get_by_role('textbox',name='新的挂灯 Token')
            expect(token).to_have_attribute('type','password')
            token.fill('bad');expect(page.get_by_role('button',name='保存连接')).to_be_disabled()
            expect(page.get_by_text('Token 应为 32 位十六进制。',exact=True)).to_be_visible()
            page.get_by_role('textbox',name='挂灯 IP 地址').fill('999.1.1.1')
            expect(page.get_by_text('IP 地址格式不正确。',exact=True)).to_be_visible()
            page.get_by_role('textbox',name='挂灯 IP 地址').fill('192.168.1.50')
            page.get_by_role('button',name='取消',exact=True).click()
            page.wait_for_timeout(450);page.screenshot(path=str(OUT/f'dc-{theme}-connection.png'))
            # Reduce motion keeps reorder and popup functional, with no transform animations.
            page.emulate_media(reduced_motion='reduce')
            page.get_by_role('button',name='调整顺序：ATK F1 鼠标').press('Home')
            assert page.locator('#dc-devs').evaluate("e=>e.getAnimations({subtree:true}).filter(a=>a.effect?.getKeyframes().some(f=>f.transform&&f.transform!=='none')).length")==0
            page.emulate_media(reduced_motion='no-preference')
            # Keep failure uses the approved alertdialog, including both choices.
            page.evaluate("syncFixture.dcModule.keep=true;syncFixture.emit('dc://module',syncFixture.dcModule);syncFixture.emit('exit-restore-error','detach 写入超时')")
            dialog=page.get_by_role('alertdialog',name='未能让模块留在后台')
            expect(dialog).to_be_visible();page.wait_for_timeout(700);page.screenshot(path=str(OUT/f'dc-{theme}-exitfail.png'))
            dialog.get_by_role('button',name='暂不退出').click();expect(dialog).to_have_count(0)
            page.evaluate("syncFixture.emit('exit-restore-error','detach 写入超时')")
            page.get_by_role('alertdialog').get_by_role('button',name='停止模块并退出').click()
            page.wait_for_function('syncFixture.exitRequested===true')
            assert page.evaluate('syncFixture.dcModule.keep') is False
            # Capability intersection hides missing write controls.
            page.evaluate("syncFixture.savedCaps=[...syncFixture.dcModule.capabilities];syncFixture.dcModule.capabilities=syncFixture.savedCaps.filter(v=>!['settings.apply','keyboard.lighting.apply','lamp.apply','mouse.mapping.read'].includes(v));syncFixture.emit('dc://module',syncFixture.dcModule)")
            expect(page.get_by_role('button',name='休眠时间',exact=True)).to_have_count(0)
            expect(page.get_by_role('button',name='键盘灯效模式',exact=True)).to_have_count(0)
            expect(page.get_by_role('switch',name='低延迟模式',exact=True)).to_have_count(0)
            page.evaluate("syncFixture.dcModule.capabilities=syncFixture.savedCaps;syncFixture.emit('dc://module',syncFixture.dcModule)")
            # Module disappearance removes DC navigation, returns to Acer.
            page.evaluate("syncFixture.setDcState('absent')")
            expect(page.locator('h1')).to_have_text('Acer 服务保护')
            expect(page.locator('.nav-sub-wrap.absent')).to_have_attribute('inert','')
            page.evaluate("syncFixture.setDcState('running')")
            page.get_by_role('button',name='服务插件',exact=True).first.click()
            expect(page.locator('h1')).to_have_text('Acer 服务保护')
            assert not errors,errors
            print('PASS DC',theme)
            ctx.close()
        browser.close()
    return 0

if __name__=='__main__':sys.exit(main())
