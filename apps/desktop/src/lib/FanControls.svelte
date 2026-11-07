<script lang="ts">
  import SegmentedControl from './ui/SegmentedControl.svelte';
  import Collapse from './ui/Collapse.svelte';
  import StatusTag from './ui/StatusTag.svelte';
  import ReadConflict from './ui/ReadConflict.svelte';
  import { writeBlocked } from './writeResources.svelte';
  type Mode = 'auto' | 'max' | 'custom';
  type Request = { mode: 'auto' | 'max' } | { mode: 'custom'; cpuPercent: number; gpuPercent: number };
  interface Props {
    mode: Mode | null;
    cpuPercent: number | null;
    gpuPercent: number | null;
    minPercent: number;
    maxPercent: number;
    canControl: boolean;
    canCustom: boolean;
    busy: boolean;
    onapply: (request: Request) => Promise<void>;
  }
  let { mode, cpuPercent, gpuPercent, minPercent, maxPercent, canControl, canCustom, busy, onapply }: Props = $props();
  const uid = $props.id();
  const modes: { value: Mode; label: string }[] = [
    { value: 'auto', label: '自动' }, { value: 'max', label: '最大' }, { value: 'custom', label: '自定义' },
  ];
  const segmentedModes = $derived(modes.map(item => ({
    ...item,
    disabled: locked || !canControl || (item.value === 'custom' && customDisabled),
  })));
  let draftMode = $state<Mode | null>(null);
  let draftCpu = $state<number | null>(null);
  let draftGpu = $state<number | null>(null);
  let dirty = $state(false);
  let submitting = $state(false);
  let submitted = $state(false);
  let error = $state('');
  const rangeValid = $derived(Number.isInteger(minPercent) && Number.isInteger(maxPercent) && minPercent >= 0 && maxPercent <= 100 && minPercent <= maxPercent);
  const midpoint = $derived(rangeValid ? Math.round((minPercent + maxPercent) / 2) : null);
  const locked = $derived(busy || submitting);
  const customDisabled = $derived(!canControl || !canCustom || !rangeValid);
  const draftValid = $derived(draftMode !== null && (draftMode !== 'custom' || (!customDisabled && validDraft(draftCpu) && validDraft(draftGpu))));
  const matchesReadback = $derived(draftMode !== null && draftMode === mode && (draftMode !== 'custom' || (validReadback(cpuPercent) && validReadback(gpuPercent) && draftCpu === cpuPercent && draftGpu === gpuPercent)));
  const pending = $derived(draftMode !== null && (dirty || !matchesReadback));
  const tag = $derived<{ text: string; kind: 'ok' | 'draft' | '' }>(
    pending ? { text: '待应用 · ' + modeLabel(draftMode), kind: 'draft' }
    : mode !== null ? { text: '当前 · ' + modeLabel(mode), kind: 'ok' }
    : { text: '模式未知', kind: '' });
  const sliders = $derived([
    { fan: 'cpu' as const, label: 'CPU', value: draftCpu },
    { fan: 'gpu' as const, label: 'GPU', value: draftGpu },
  ]);

  function validReadback(value: number | null): value is number {
    return value !== null && Number.isFinite(value) && value >= 0 && value <= 100;
  }
  function validDraft(value: number | null): value is number {
    return rangeValid && value !== null && Number.isInteger(value) && value >= minPercent && value <= maxPercent;
  }
  function readback(value: number | null) { return validReadback(value) ? `${value}%` : '未知'; }
  function draftText(value: number | null) { return validDraft(value) ? `${value}%` : '—'; }
  function modeLabel(value: Mode | null) { return modes.find(item => item.value === value)?.label ?? '未知'; }
  function fill(value: number | null) {
    if (!rangeValid || value === null || !Number.isFinite(value)) return '0%';
    if (maxPercent === minPercent) return '100%';
    return Math.min(100, Math.max(0, (value - minPercent) / (maxPercent - minPercent) * 100)) + '%';
  }
  $effect(() => {
    if (!dirty) {
      draftMode = mode;
      draftCpu = validReadback(cpuPercent) ? cpuPercent : midpoint;
      draftGpu = validReadback(gpuPercent) ? gpuPercent : midpoint;
    }
  });
  $effect(() => { if (dirty && matchesReadback) dirty = false; });

  function choose(value: Mode) {
    if (locked || !canControl || (value === 'custom' && customDisabled)) return;
    draftMode = value;
    if (value === 'custom') {
      if (!validDraft(draftCpu)) draftCpu = midpoint;
      if (!validDraft(draftGpu)) draftGpu = midpoint;
    }
    dirty = true;
    submitted = false;
    error = '';
  }
  function edit(fan: 'cpu' | 'gpu', event: Event & { currentTarget: EventTarget & HTMLInputElement }) {
    if (locked || customDisabled) return;
    const value = event.currentTarget.valueAsNumber;
    if (fan === 'cpu') draftCpu = Number.isFinite(value) ? value : null;
    else draftGpu = Number.isFinite(value) ? value : null;
    dirty = true;
    submitted = false;
    error = '';
  }
  async function apply() {
    if (locked || writeBlocked('performance') || !canControl || !draftValid || !draftMode) return;
    const request: Request = draftMode === 'custom'
      ? { mode: 'custom', cpuPercent: draftCpu!, gpuPercent: draftGpu! }
      : { mode: draftMode };
    submitting = true;
    error = '';
    submitted = false;
    try {
      await onapply(request);
      submitted = true;
    } catch (cause) {
      error = cause instanceof Error ? cause.message : typeof cause === 'string' ? cause : '风扇设置请求失败，请查看操作记录。';
    } finally { submitting = false; }
  }
</script>

<div class="sh"><span>风扇控制</span><StatusTag text={tag.text} kind={tag.kind}/></div>
<section class="group fan-controls" aria-label="风扇控制" aria-busy={submitting}>
  <div class="row mode-row">
    <span class="row-l" aria-live="polite">
      <b>风扇模式</b>
      <small>当前读回 · {modeLabel(mode)}{#if validReadback(cpuPercent) || validReadback(gpuPercent)} · {mode === 'custom' ? '目标比例' : '上次自定义目标'} CPU {readback(cpuPercent)} / GPU {readback(gpuPercent)}{/if}</small>
    </span>
    <div class="mode-control">
      <SegmentedControl value={draftMode ?? ''} current={mode ?? undefined} options={segmentedModes} onchange={(value) => choose(value as Mode)} label="选择风扇控制方式" disabled={locked || !canControl}/>
    </div>
  </div>
  <Collapse open={draftMode === 'custom' && rangeValid}>
    <div class="custom-rows">
      {#each sliders as item (item.fan)}
        <div class="slider-row">
          <label for={uid + '-' + item.fan}>{item.label}</label>
          <input class="slider" id={uid + '-' + item.fan} type="range" min={minPercent} max={maxPercent} step="1" style:--p={fill(validDraft(item.value) ? item.value : midpoint)} value={validDraft(item.value) ? item.value : midpoint ?? undefined} disabled={locked || customDisabled} aria-describedby={uid + '-draft'} oninput={(event) => edit(item.fan, event)}/>
          <span class="num-field"><input class="num" class:invalid={!validDraft(item.value)} type="number" aria-label={item.label + ' 风扇百分比数值'} aria-invalid={!validDraft(item.value)} min={minPercent} max={maxPercent} step="1" value={item.value ?? ''} disabled={locked || customDisabled} aria-describedby={uid + '-draft'} oninput={(event) => edit(item.fan, event)}/><span aria-hidden="true">%</span></span>
        </div>
      {/each}
      {#if rangeValid}<p class="hint range-note">可调范围 {minPercent}–{maxPercent}%。</p>{/if}
    </div>
  </Collapse>
  <ReadConflict readKey={mode ? JSON.stringify([mode, mode === 'custom' ? cpuPercent : null, mode === 'custom' ? gpuPercent : null]) : null} edited={dirty} matches={matchesReadback} label={modeLabel(mode) + (mode === 'custom' ? ` · CPU ${readback(cpuPercent)} / GPU ${readback(gpuPercent)}` : '')} onadopt={() => { draftMode = mode; draftCpu = validReadback(cpuPercent) ? cpuPercent : midpoint; draftGpu = validReadback(gpuPercent) ? gpuPercent : midpoint; dirty = false; }}/>
  <div class="row foot">
    <span id={uid + '-draft'} class="hint" aria-live="polite">
      {#if !canControl}连接设备并获取控制权限后可调整。
      {:else if !rangeValid}尚无有效调节范围，自定义暂不可用。
      {:else if !canCustom && draftMode === 'custom'}自定义控制尚未开放。
      {:else if draftMode === 'custom'}{pending ? `待应用：自定义 · CPU ${draftText(draftCpu)} / GPU ${draftText(draftGpu)}` : '与当前设置一致'}{!validReadback(cpuPercent) || !validReadback(gpuPercent) ? '；未知目标暂以范围中值编辑。' : ''}{!validDraft(draftCpu) || !validDraft(draftGpu) ? ' 请输入范围内的整数。' : ''}
      {:else if dirty}待应用：{modeLabel(draftMode)}
      {:else if mode}与当前设置一致。
      {:else}当前模式未知，请选择待应用模式。{/if}
    </span>
    <button class="btn primary" type="button" disabled={locked || writeBlocked('performance') || !canControl || !draftValid} onclick={apply}>{#if submitting}<span class="spinner" aria-hidden="true"></span>应用中…{:else}应用{/if}</button>
  </div>
  {#if error}<div class="row msg"><p class="hint error" role="alert">{error}</p></div>{:else if submitted}<div class="row msg"><p class="hint" role="status">{matchesReadback ? '设备读回与所选设置一致。' : '请求已返回，尚未读回所选设置；以设备状态与操作记录为准。'}</p></div>{/if}
</section>

<style>
  .fan-controls { min-width:0; }
  .mode-control { flex:none; min-width:0; }
  .custom-rows { padding:6px 0 10px; }
  .slider-row { display:grid; grid-template-columns:72px minmax(64px,1fr) auto; align-items:center; gap:14px; padding:10px 18px; }
  .slider-row label { font-size:13px; font-weight:500; }
  .num-field { display:inline-flex; align-items:center; gap:6px; font-size:12px; color:var(--label-2); }
  .num-field .num { width:64px; }
  .range-note { margin:2px 18px 0; }
  .msg { min-height:0; padding-top:10px; padding-bottom:10px; }
  .msg p { margin:0; overflow-wrap:anywhere; }
  .error { color:var(--red-text); }
  @media (max-width:560px) {
    .mode-row { flex-wrap:wrap; }
    .mode-control { width:100%; }
    .slider-row { grid-template-columns:40px minmax(64px,1fr) auto; gap:10px; }
  }
</style>
