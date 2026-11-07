<script lang="ts">
  // Draft-only edits; one explicit apply writes both fields. Only a separate query confirms.
  import { onDestroy, untrack } from 'svelte';
  import { api, errorText, type UsbChargeState, type UsbChargeReadback, type UsbChargeThreshold } from './api';
  import Switch from './ui/Switch.svelte';
  import SegmentedControl from './ui/SegmentedControl.svelte';
  import StatusTag from './ui/StatusTag.svelte';
  import ReadConflict from './ui/ReadConflict.svelte';
  import { beginWrite, writeBlocked } from './writeResources.svelte';
  let { authorized, refreshToken = 0 }: { authorized: boolean; refreshToken?: number } = $props();
  const uid = $props.id();
  const thresholds: UsbChargeThreshold[] = [10, 20, 30];
  let readback = $state<UsbChargeReadback | null>(null);
  let draft = $state<UsbChargeState | null>(null);
  let draftEdited = $state(false);
  let reading = $state(false);
  let writing = $state(false);
  let fresh = $state(false);
  let error = $state('');
  let result = $state<{ confirmed: boolean; message: string } | null>(null);
  let disposed = false;
  let refreshQueued = false;
  let readPromise: Promise<void> | null = null;
  let serial = 0;
  const locked = $derived(writeBlocked('usb'));
  const editable = $derived(authorized && fresh && !locked && readback?.writable === true && readback.ack === 'confirmed' && !readback.error && validState(readback.state));
  const dirty = $derived(draft !== null && (!validState(readback?.state) || !same(draft, readback!.state!)));
  const tag = $derived<{ text: string; kind: 'ok' | 'draft' | 'er' | '' }>(
    !authorized ? { text: '等待授权', kind: '' }
    : writing ? { text: '应用中', kind: 'draft' }
    : reading ? { text: '读取中', kind: '' }
    : !fresh ? { text: '状态未知', kind: error ? 'er' : '' }
    : dirty ? { text: '待应用草稿', kind: 'draft' }
    : { text: '与设备读回一致', kind: 'ok' }
  );

  function validState(value: unknown): value is UsbChargeState {
    if (!value || typeof value !== 'object') return false;
    const candidate = value as Partial<UsbChargeState>;
    return typeof candidate.enabled === 'boolean' && thresholds.includes(candidate.threshold as UsbChargeThreshold);
  }
  function same(a: UsbChargeState, b: UsbChargeState) { return a.enabled === b.enabled && a.threshold === b.threshold; }
  function validate(value: UsbChargeReadback) {
    if (!value || value.backend !== 'direct_wmi' || !['confirmed', 'rejected', 'unknown'].includes(value.ack)) throw new Error('USB关机供电返回不完整，当前状态未知。');
    return value;
  }
  function confirmedRead(value: UsbChargeReadback) { return value.ack === 'confirmed' && !value.error && validState(value.state); }
  function acceptRead(value: UsbChargeReadback) {
    readback = value;
    fresh = confirmedRead(value);
    if (fresh && validState(value.state) && (!draftEdited || (draft && same(draft, value.state)))) {
      draft = { ...value.state };
      draftEdited = false;
    }
    if (!fresh) error = value.error || (value.ack === 'rejected' ? '设备未接受USB关机供电状态查询。' : 'USB关机供电状态尚未确认，请刷新后操作。');
  }
  function refresh(): Promise<void> {
    if (!authorized) { serial++; fresh = false; error = ''; result = null; return Promise.resolve(); }
    if (readPromise) return readPromise;
    if (disposed) return Promise.resolve();
    if (writing) { refreshQueued = true; return Promise.resolve(); }
    const request = ++serial;
    reading = true; error = ''; result = null;
    readPromise = (async () => { try {
      const next = validate(await api.getUsbCharge());
      if (!disposed && authorized && request === serial) acceptRead(next);
    } catch (cause) { if (!disposed && request === serial) { fresh = false; error = 'USB关机供电读取失败：' + errorText(cause); } }
    finally {
      readPromise = null;
      if (!disposed) {
        reading = false;
        if (refreshQueued) { refreshQueued = false; void refresh(); }
      }
    }
    })();
    return readPromise;
  }
  function editEnabled(event: Event & { currentTarget: EventTarget & HTMLInputElement }) {
    if (!editable || !draft) return;
    draft = { ...draft, enabled: event.currentTarget.checked };
    draftEdited = true; error = ''; result = null;
  }
  function editThreshold(value: string) {
    if (!editable || !draft) return;
    const threshold = Number(value) as UsbChargeThreshold;
    if (!thresholds.includes(threshold)) return;
    // Threshold edits deliberately retain draft.enabled, including false.
    draft = { ...draft, threshold };
    draftEdited = true; error = ''; result = null;
  }
  async function apply() {
    if (!editable || !dirty || !validState(draft)) return;
    const release = beginWrite('usb');
    if (!release) return;
    ++serial;
    const request = { ...draft };
    writing = true; fresh = false; error = ''; result = null;
    try {
      const outcome = await api.setUsbCharge(request);
      if (disposed) return;
      if (outcome.status === 'failed') throw new Error(outcome.message || 'USB关机供电设置失败。');
      const next = validate(await api.getUsbCharge());
      if (disposed) return;
      readback = next;
      const confirmed = outcome.status === 'applied' && confirmedRead(next) && validState(next.state) && same(next.state, request);
      fresh = confirmed;
      if (confirmed) { draft = { ...next.state! }; draftEdited = false; }
      result = {
        confirmed,
        message: confirmed ? 'USB关机供电设置已读回确认。物理关机供电效果仍待测试。' : 'USB关机供电设置未确认：' + (next.error || outcome.message || '查询读回与请求不一致，请刷新后检查。'),
      };
    } catch (cause) { if (!disposed) error = 'USB关机供电设置失败，状态已过期：' + errorText(cause); }
    finally {
      release();
      if (!disposed) {
        writing = false;
        if (refreshQueued) { refreshQueued = false; void refresh(); }
      }
    }
  }
  $effect(() => { void refreshToken; void authorized; untrack(() => { void refresh(); }); });
  onDestroy(() => { disposed = true; });
</script>

{#snippet busyLabel(active: boolean, busyText: string, idleText: string)}{#if active}<span class="spinner" aria-hidden="true"></span>{busyText}{:else}{idleText}{/if}{/snippet}

<div class="sh"><span>USB 关机供电</span><span><StatusTag text={tag.text} kind={tag.kind}/></span></div>
<div class="group" role="group" aria-label="USB关机供电" aria-busy={reading || writing}>
  <div class="row">
    <span class="row-l"><b>{!authorized ? '等待授权' : fresh ? '当前设置' : '上次 / 未确认'}</b><small>控制关机后的USB供电，独立于电池充电上限保护。物理关机供电效果仍待测试。</small></span>
    <span class="row-v" aria-live="polite">{validState(readback?.state) ? (readback!.state!.enabled ? '开启' : '关闭') + ' · 截止电量 ' + readback!.state!.threshold + '%' : '未知'}</span>
    <button type="button" class="btn" disabled={!authorized} aria-busy={reading} onclick={refresh}>{@render busyLabel(reading, '读取中…', '刷新状态')}</button>
  </div>
  <div class="row">
    <label class="row-l" for={uid + '-enabled'}><b>启用USB关机供电</b><small>修改只编辑草稿，点击应用后再读取确认。</small></label>
    <Switch id={uid + '-enabled'} checked={draft?.enabled ?? false} disabled={!editable} pending={writing} onchange={editEnabled}/>
  </div>
  <div class="row">
    <span class="row-l"><b>电量截止阈值</b><small>低于该电量停止关机供电；修改阈值不会自动开启功能。</small></span>
    <SegmentedControl size="small" label="电量截止阈值" value={draft ? String(draft.threshold) : ''} disabled={!editable} options={thresholds.map(threshold => ({ value: String(threshold), label: threshold + '%' }))} onchange={editThreshold}/>
  </div>
  {#if error}<div class="row"><p class="hint er" role="alert">{error}</p></div>{:else if result}<div class="row"><p class="hint" class:ok-text={result.confirmed} role="status">{result.message}</p></div>{/if}
  {#if !authorized}<div class="row"><p class="hint">请先授权设备访问，再修改USB设置。</p></div>{:else if !fresh && !reading && !writing}<div class="row"><p class="hint">当前状态未知或已过期，已暂停编辑，请刷新后操作。</p></div>{:else if fresh && readback?.writable !== true}<div class="row"><p class="hint">当前设备未开放USB关机供电写入。</p></div>{/if}
  <ReadConflict readKey={fresh && readback?.state ? JSON.stringify(readback.state) : null} edited={draftEdited} matches={!dirty} label={readback?.state ? (readback.state.enabled ? '开启' : '关闭') + ' / ' + readback.state.threshold + '%' : '—'} onadopt={() => { if (readback?.state) draft = { ...readback.state }; draftEdited = false; }}/>
  <div class="row foot">
    <span class="hint" aria-live="polite">{draft ? !fresh ? '上次草稿（状态未确认）' : dirty ? '待应用草稿：' + (draft.enabled ? '开启' : '关闭') + ' / 截止电量 ' + draft.threshold + '%' : '与设备读回一致' : '读取确认后可编辑草稿'}</span>
    <button type="button" class="btn primary" disabled={!editable || !dirty || !validState(draft)} onclick={apply}>{@render busyLabel(writing, '应用中…', '应用USB设置')}</button>
  </div>
</div>

<style>
  p.hint {margin:0;overflow-wrap:anywhere;}
  .er {color:var(--red-text, var(--danger));}
  .ok-text {color:var(--green-text, var(--success));}
  label.row-l {cursor:pointer;}
  .row > .btn {flex-shrink:0;}
  @media(max-width:520px) {.row {flex-wrap:wrap;}}
</style>
