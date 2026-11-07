<script lang="ts">
  // Query on entry or an explicit parent revision, never with the sensor polling timer.
  // Keep the last confirmed group readable during a single-item write/readback.
  // Other switches stay writable; each item owns its lock and stale result separately.
  import { onDestroy, untrack } from 'svelte';
  import { api, errorText, type VendorControlsState, type VendorSetting } from './api';
  import Switch from './ui/Switch.svelte';
  import StatusTag from './ui/StatusTag.svelte';
  import { beginWrite, writeBlocked } from './writeResources.svelte';
  let { authorized, refreshToken = 0 }: { authorized: boolean; refreshToken?: number } = $props();
  const uid = $props.id();
  const controls: { setting: VendorSetting; label: string; hint?: string }[] = [
    { setting: 'lcd_overdrive', label: 'LCD响应加速' },
    { setting: 'boot_sound', label: '启动声音', hint: '开关状态已验证；重启时的声音效果尚待实测。' },
    { setting: 'sticky_key', label: '粘滞键', hint: '控制当前 Windows 用户，同时切换连按 Shift 快捷键。' },
    { setting: 'backlight_timeout', label: '闲置30秒关闭键盘背光' },
  ];
  let vendorState = $state<VendorControlsState | null>(null);
  let reading = $state(false);
  let writes = $state<VendorSetting[]>([]);
  let stale = $state<VendorSetting[]>([]);
  let fresh = $state(false);
  let error = $state('');
  let result = $state<{ confirmed: boolean; message: string } | null>(null);
  let disposed = false;
  let refreshQueued = false;
  let readPromise: Promise<void> | null = null;
  let serial = 0;
  const tag = $derived<{ text: string; kind: 'ok' | 'draft' | 'er' | '' }>(
    writes.length ? { text: '等待读回 · 其余为上次值', kind: 'draft' }
    : reading ? { text: '读取中', kind: '' }
    : error ? { text: '状态未知', kind: 'er' }
    : fresh ? { text: '与设备读回一致', kind: 'ok' }
    : { text: '状态未知', kind: '' }
  );

  function itemFor(setting: VendorSetting) { return vendorState?.items.find(item => item.setting === setting); }
  function writable(setting: VendorSetting) {
    const item = itemFor(setting);
    return (authorized || setting === 'sticky_key') && fresh && !stale.includes(setting) && !writeBlocked(`vendor:${setting}`) && item?.writable === true && typeof item.enabled === 'boolean' && !item.error;
  }
  function validate(value: VendorControlsState) {
    if (!value || !['official_service', 'mixed'].includes(value.backend) || !Array.isArray(value.items)) throw new Error('厂商设置返回不完整，当前状态未知。');
    return value;
  }
  function refresh(): Promise<void> {
    if (readPromise) return readPromise;
    if (disposed) return Promise.resolve();
    if (writes.length) { refreshQueued = true; return Promise.resolve(); }
    const request = ++serial;
    reading = true;
    error = '';
    result = null;
    readPromise = (async () => { try {
      const next = validate(await api.getVendorControls());
      if (disposed || request !== serial) return;
      vendorState = next;
      fresh = true;
      stale = [];
    } catch (cause) {
      if (!disposed && request === serial) { fresh = false; error = '厂商设置读取失败：' + errorText(cause); }
    } finally {
      readPromise = null;
      if (!disposed) {
        reading = false;
        if (refreshQueued) { refreshQueued = false; void refresh(); }
      }
    }
    })();
    return readPromise;
  }
  async function change(setting: VendorSetting, event: Event & { currentTarget: EventTarget & HTMLInputElement }) {
    const enabled = event.currentTarget.checked;
    event.currentTarget.checked = itemFor(setting)?.enabled === true;
    if (!writable(setting)) return;
    const release = beginWrite(`vendor:${setting}`);
    if (!release) return;
    writes = [...writes, setting];
    ++serial;
    error = '';
    result = null;
    const label = controls.find(item => item.setting === setting)!.label;
    try {
      const outcome = await api.setVendorControl(setting, enabled);
      if (disposed) return;
      if (outcome.status === 'failed') throw new Error(outcome.message || label + '设置失败。');
      const next = validate(await api.getVendorControls());
      if (disposed) return;
      // Concurrent item writes may return whole-group snapshots out of order.
      // Merge only this action's item, retaining other confirmed items until their own readback.
      if (vendorState) vendorState = { ...vendorState, items: vendorState.items.map(item => item.setting === setting ? next.items.find(row => row.setting === setting) ?? item : item) };
      const actual = next.items.find(item => item.setting === setting);
      const confirmed = outcome.status === 'applied' && actual?.enabled === enabled && !actual.error;
      stale = confirmed ? stale.filter(item => item !== setting) : [...stale.filter(item => item !== setting), setting];
      result = {
        confirmed,
        message: confirmed ? `${label}已${enabled ? '开启' : '关闭'}，设备读回一致。` : `${label}设置结果未确认。${actual?.error || outcome.message || '读回与请求不一致，请刷新状态。'}`,
      };
    } catch (cause) {
      if (!disposed) { stale = [...stale.filter(item => item !== setting), setting]; error = `${label}设置失败，当前状态已过期：${errorText(cause)}`; }
    } finally {
      release();
      if (!disposed) {
        writes = writes.filter(item => item !== setting);
        if (refreshQueued) { refreshQueued = false; void refresh(); }
      }
    }
  }
  $effect(() => { void refreshToken; void authorized; untrack(() => { void refresh(); }); });
  onDestroy(() => { disposed = true; });
</script>

<div class="sh"><span>厂商外设</span><span><StatusTag text={tag.text} kind={tag.kind}/></span></div>
<div class="group" role="group" aria-label="厂商设备设置" aria-busy={reading || writes.length > 0}>
  <div class="row">
    <span class="row-l"><small>{vendorState?.dependency || '读取设备设置…'}</small></span>
    <button type="button" class="btn" aria-busy={reading} onclick={refresh}>{#if reading}<span class="spinner" aria-hidden="true"></span>读取中…{:else}刷新状态{/if}</button>
  </div>
  {#each controls as control}
    {@const item = itemFor(control.setting)}
    {@const writing = writes.includes(control.setting) ? control.setting : null}
    <div class="row">
      <label class="row-l" for={uid + '-' + control.setting}><b>{control.label}</b>{#if control.hint}<small>{control.hint}</small>{/if}{#if item?.error}<small class="er">{item.error}</small>{/if}</label>
      <span class="row-v">{writing === control.setting ? '等待读回…' : typeof item?.enabled === 'boolean' ? (fresh && !reading && !writes.length && !stale.includes(control.setting) && !item.error ? '' : '上次：') + (item.enabled ? '开启' : '关闭') : '—'}</span>
      <Switch id={uid + '-' + control.setting} checked={item?.enabled === true} disabled={!writable(control.setting)} pending={writing === control.setting} onchange={event => change(control.setting, event)}/>
    </div>
  {/each}
  {#if error}<div class="row"><p class="hint er" role="alert">{error}</p></div>{:else if result}<div class="row"><p class="hint" class:ok-text={result.confirmed} role="status">{result.message}</p></div>{/if}
  {#if !authorized}<div class="row foot"><p class="hint">粘滞键可直接修改；其余设备设置需要先授权。</p></div>{:else if !fresh && !reading && !writes.length}<div class="row foot"><p class="hint">当前状态未知或已过期，请刷新后操作。</p></div>{/if}
</div>

<style>
  p.hint { margin:0; overflow-wrap:anywhere; }
  .er, .row-l small.er { color:var(--red-text, var(--danger)); }
  .ok-text { color:var(--green-text, var(--success)); }
  label.row-l { cursor:pointer; }
  .row-v { font-size:12px; white-space:nowrap; }
  .row > .btn { flex-shrink:0; }
  @media(max-width:420px) { .row { gap:10px; } .row-v { max-width:64px; white-space:normal; } }
</style>
