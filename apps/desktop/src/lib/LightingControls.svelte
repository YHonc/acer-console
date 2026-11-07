<script lang="ts">
  import ReadConflict from './ui/ReadConflict.svelte';
  import { writeBlocked } from './writeResources.svelte';
  interface Props {
    zones: string[] | null;
    brightness: number | null;
    canControl: boolean;
    busy: boolean;
    onapply: (request: { zones: string[]; brightness: number }) => Promise<void>;
    pending?: boolean;
    resetRevision?: number;
  }
  let { zones, brightness, canControl, busy, onapply, pending = $bindable(false), resetRevision = 0 }: Props = $props();
  const uid = $props.id();
  const zoneIndices = [0, 1, 2, 3];
  let draftZones = $state<(string | null)[]>([null, null, null, null]);
  let fillColor = $state('#ffffff');
  let draftBrightness = $state<number | null>(50);
  let dirty = $state(false);
  let submitting = $state(false);
  let submitted = $state(false);
  let error = $state('');
  let lastReset = 0;
  $effect(() => { if (resetRevision !== lastReset) { lastReset = resetRevision; dirty = false; } });
  const locked = $derived(busy || submitting);
  const knownZones = $derived(validZones(zones));
  const knownBrightness = $derived(validBrightness(brightness));
  const draftValid = $derived(validZones(draftZones) && validBrightness(draftBrightness));
  const matchesReadback = $derived(knownZones && knownBrightness && brightness === draftBrightness && zones!.every((color, index) => color.toLowerCase() === draftZones[index]?.toLowerCase()));

  function validColor(value: unknown): value is string { return typeof value === 'string' && /^#[\da-f]{6}$/i.test(value); }
  function validZones(value: unknown): value is string[] { return Array.isArray(value) && value.length === 4 && value.every(validColor); }
  function validBrightness(value: number | null): value is number { return value !== null && Number.isInteger(value) && value >= 0 && value <= 100; }
  $effect(() => {
    if (!dirty) {
      draftZones = validZones(zones) ? [...zones] : [null, null, null, null];
      draftBrightness = validBrightness(brightness) ? brightness : 50;
    }
  });
  $effect(() => { if (dirty && matchesReadback) dirty = false; });
  $effect(() => { pending = dirty || !matchesReadback; });

  function markDraft() { dirty = true; submitted = false; error = ''; }
  function editZone(index: number, event: Event & { currentTarget: EventTarget & HTMLInputElement }) {
    if (locked || !canControl) return;
    draftZones[index] = event.currentTarget.value;
    markDraft();
  }
  function editBrightness(event: Event & { currentTarget: EventTarget & HTMLInputElement }) {
    if (locked || !canControl) return;
    const value = event.currentTarget.valueAsNumber;
    draftBrightness = Number.isFinite(value) ? value : null;
    markDraft();
  }
  function fillZones() {
    if (locked || !canControl || !validColor(fillColor)) return;
    draftZones = [fillColor, fillColor, fillColor, fillColor];
    markDraft();
  }
  async function apply() {
    if (locked || writeBlocked('lighting') || !canControl || !draftValid) return;
    const request = { zones: draftZones.map(color => color!), brightness: draftBrightness! };
    submitting = true;
    submitted = false;
    error = '';
    try {
      await onapply(request);
      submitted = true;
    } catch (cause) {
      error = cause instanceof Error ? cause.message : typeof cause === 'string' ? cause : '灯光设置请求失败，请查看操作记录。';
    } finally { submitting = false; }
  }
</script>

<div class="lighting-controls" role="group" aria-label="四区静态灯光" aria-busy={submitting}>
  <div class="row zone-head">
    <span class="row-l"><b>四区颜色</b><small>从左到右对应键盘四个分区 · {matchesReadback ? '设备读回颜色示意，实际效果以键盘为准。' : '待应用草稿预览，不代表当前键盘灯光。'}</small></span>
  </div>
  <div class="zones">
    {#each zoneIndices as index}
      <label class="zone" class:unknown={!validColor(draftZones[index])} for={uid + '-zone-' + index}>
        <span>{index + 1}区</span>
        <input id={uid + '-zone-' + index} type="color" value={draftZones[index] ?? '#ffffff'} disabled={locked || !canControl} aria-invalid={dirty && !validColor(draftZones[index])} oninput={event => editZone(index, event)}/>
        <span class="code">{validColor(draftZones[index]) ? draftZones[index]!.toUpperCase() : '未选择'}</span>
      </label>
    {/each}
  </div>
  <div class="row">
    <span class="row-l"><b>统一颜色</b><small>仅修改草稿</small></span>
    <input class="fill-color" type="color" aria-label="统一颜色" bind:value={fillColor} disabled={locked || !canControl}/>
    <button type="button" class="btn" disabled={locked || !canControl} onclick={fillZones}>填充四区</button>
  </div>
  <div class="row slider-line">
    <label for={uid + '-brightness'}>亮度</label>
    <input class="slider" id={uid + '-brightness'} type="range" min="0" max="100" step="1" style:--p={(validBrightness(draftBrightness) ? draftBrightness : 50) + '%'} value={validBrightness(draftBrightness) ? draftBrightness : 50} disabled={locked || !canControl} aria-describedby={uid + '-draft'} oninput={editBrightness}/>
    <span class="num-field"><input class="num" class:invalid={!validBrightness(draftBrightness)} type="number" min="0" max="100" step="1" value={draftBrightness ?? ''} aria-label="灯光亮度百分比数值" aria-invalid={!validBrightness(draftBrightness)} aria-describedby={uid + '-draft'} disabled={locked || !canControl} oninput={editBrightness}/><span aria-hidden="true">%</span></span>
  </div>
  <div class="row">
    <span class="row-l"><b>静态读数</b><small>亮度 {knownBrightness ? brightness + '%' : '未知'}</small></span>
    <span class="readback" aria-label="设备实际颜色读回">
      {#if knownZones}{#each zoneIndices as index}<span class="swatch" style:background={zones![index]} title={`${index + 1}区 ${zones![index].toUpperCase()}`}></span>{/each}<span class="sr-only">{zones!.map((color, index) => `${index + 1}区 ${color.toUpperCase()}`).join('，')}</span>{:else}<span class="hint">四区颜色未知</span>{/if}
    </span>
  </div>
  <ReadConflict readKey={knownZones && knownBrightness ? JSON.stringify([zones, brightness]) : null} edited={dirty} matches={matchesReadback} label={knownZones && knownBrightness ? zones!.join(' / ') + ` · ${brightness}%` : '—'} onadopt={() => { draftZones = zones ? [...zones] : [null, null, null, null]; draftBrightness = knownBrightness ? brightness : 50; dirty = false; }}/>
  <div class="row foot">
    <span id={uid + '-draft'} class="hint" aria-live="polite">{!canControl ? '静态灯光控制暂不可用' : dirty || !matchesReadback ? '待应用草稿' : '与当前设置一致'}。{!validZones(draftZones) ? '请明确选择全部四区颜色，也可用统一颜色填充。' : ''}{!knownBrightness ? '亮度初始草稿为50%，不是设备读数。' : ''}{!validBrightness(draftBrightness) ? '亮度须为0至100的整数。' : ''}</span>
    <button class="btn primary" type="button" disabled={locked || writeBlocked('lighting') || !canControl || !draftValid} onclick={apply}>{#if submitting}<span class="spinner" aria-hidden="true"></span>应用中…{:else}应用静态灯光{/if}</button>
  </div>
  {#if error}<div class="row msg"><p class="hint err-text" role="alert">{error}</p></div>{:else if submitted}<div class="row msg"><p class="hint" role="status">{matchesReadback ? '设备读回与所选灯光一致。' : '请求已返回，尚未读回所选灯光；以设备状态与操作记录为准。'}</p></div>{/if}
</div>

<style>
  .lighting-controls { min-width:0; }
  .zone-head { min-height:40px; padding-bottom:4px; }
  .zones { display:grid; grid-template-columns:repeat(4,minmax(0,1fr)); gap:12px; padding:6px 18px 16px; }
  .zone { display:flex; flex-direction:column; gap:6px; min-width:0; font-size:12px; color:var(--label-2); }
  .zone input { -webkit-appearance:none; appearance:none; width:100%; height:36px; border:0; border-radius:9px; padding:0; cursor:pointer; background:none; box-shadow:inset 0 0 0 .5px rgba(0,0,0,.1); }
  .zone input::-webkit-color-swatch-wrapper { padding:0; }
  .zone input::-webkit-color-swatch { border:0; border-radius:9px; }
  .zone input::-moz-color-swatch { border:0; border-radius:9px; }
  .zone.unknown input { opacity:.35; }
  .zone input:disabled { cursor:default; opacity:.4; }
  .zone input[aria-invalid="true"] { box-shadow:0 0 0 1.5px var(--red); }
  .code { font-variant-numeric:tabular-nums; font-size:11px; }
  .fill-color { width:30px; height:30px; padding:0; border:0; border-radius:50%; background:none; cursor:pointer; }
  .fill-color::-webkit-color-swatch-wrapper { padding:0; }
  .fill-color::-webkit-color-swatch { border:0; border-radius:50%; box-shadow:inset 0 0 0 .5px rgba(0,0,0,.15); }
  .fill-color:disabled { opacity:.4; cursor:default; }
  .slider-line { display:grid; grid-template-columns:72px minmax(64px,1fr) auto; gap:14px; }
  .slider-line label { font-size:13px; font-weight:500; }
  .num-field { display:inline-flex; align-items:center; gap:6px; font-size:12px; color:var(--label-2); }
  .num-field .num { width:64px; }
  .readback { display:inline-flex; align-items:center; gap:6px; }
  .swatch { width:16px; height:16px; border-radius:50%; box-shadow:inset 0 0 0 .5px rgba(0,0,0,.15); }
  .sr-only { position:absolute; width:1px; height:1px; padding:0; margin:-1px; overflow:hidden; clip:rect(0,0,0,0); white-space:nowrap; border:0; }
  .msg { min-height:0; padding-top:10px; padding-bottom:10px; }
  .msg p { margin:0; overflow-wrap:anywhere; }
  @media (max-width:560px) { .zones { grid-template-columns:repeat(2,minmax(0,1fr)); } .slider-line { grid-template-columns:40px minmax(64px,1fr) auto; gap:10px; } }
</style>
