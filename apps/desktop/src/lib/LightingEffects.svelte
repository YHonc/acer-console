<script lang="ts">
  import { onDestroy } from 'svelte';
  import LightingControls from './LightingControls.svelte';
  import SegmentedControl from './ui/SegmentedControl.svelte';
  import Collapse from './ui/Collapse.svelte';
  import StatusTag from './ui/StatusTag.svelte';
  import ReadConflict from './ui/ReadConflict.svelte';
  import { writeBlocked } from './writeResources.svelte';
  import type { LightingConfig, LightingEffect, LightingAnimation } from './api';
  interface Props {
    config: LightingConfig | null;
    canStatic: boolean;
    canDynamic: boolean;
    canRestore: boolean;
    busy: boolean;
    restoreReason?: string | null;
    onstatic: (request: { zones: string[]; brightness: number }) => Promise<void>;
    onanimation: (request: LightingAnimation) => Promise<void>;
    onrestore: () => Promise<void>;
  }
  let { config, canStatic, canDynamic, canRestore, busy, restoreReason = null, onstatic, onanimation, onrestore }: Props = $props();
  const uid = $props.id();
  const effects: { value: LightingEffect; label: string; preview: string }[] = [
    { value: 'static', label: '常亮 · 四区静态', preview: 'p-static' }, { value: 'breathing', label: '呼吸', preview: 'p-breath' },
    { value: 'neon', label: '霓虹', preview: 'p-neon' }, { value: 'wave', label: '波浪', preview: 'p-wave' },
    { value: 'shifting', label: '渐变', preview: 'p-shift' }, { value: 'zoom', label: '缩放', preview: 'p-zoom' },
    { value: 'meteor', label: '流星', preview: 'p-meteor' }, { value: 'twinkling', label: '闪烁', preview: 'p-flash' },
  ];
  const palette = ['#ff3b30', '#ff9500', '#ffcc00', '#34c759', '#0a84ff', '#5e5ce6', '#ffffff'];
  let draftEffect = $state<LightingEffect | null>(null);
  let draftSpeed = $state<number | null>(5);
  let draftBrightness = $state<number | null>(50);
  let draftDirection = $state<1 | 2>(1);
  let draftColor = $state('#ffffff');
  let staticPending = $state(false);
  let adoptRevision = $state(0);
  let dirty = $state(false);
  let submitting = $state(false);
  let restoring = $state(false);
  let submitted = $state(false);
  let error = $state('');
  let note = $state('');
  let disposed = false;
  const locked = $derived(busy || submitting || restoring);
  const hasDirection = $derived(draftEffect === 'wave' || draftEffect === 'shifting');
  const ignoresColor = $derived(draftEffect === 'neon' || draftEffect === 'wave');
  const dynamicValid = $derived(draftEffect !== null && draftEffect !== 'static' && validInteger(draftSpeed, 1, 9) && validInteger(draftBrightness, 0, 100) && validColor(draftColor));
  const matchesReadback = $derived(config !== null && config.effect === draftEffect && (draftEffect === 'static' || (
    config.speed === draftSpeed && config.brightness === draftBrightness &&
    (!hasDirection || config.direction === draftDirection) &&
    (ignoresColor || (validColor(config.color) && config.color.toLowerCase() === draftColor.toLowerCase()))
  )));
  const draftPending = $derived(draftEffect === 'static' ? dirty || staticPending : dirty || !matchesReadback);
  const tag = $derived<{ text: string; kind: 'ok' | 'draft' | '' }>(
    !config ? { text: '状态未知', kind: '' }
    : draftPending ? { text: '待应用草稿', kind: 'draft' }
    : { text: '与设备读回一致', kind: 'ok' });
  const paletteChoice = $derived(palette.some(color => color === draftColor.toLowerCase()));

  function validInteger(value: number | null, min: number, max: number): value is number { return value !== null && Number.isInteger(value) && value >= min && value <= max; }
  function validColor(value: unknown): value is string { return typeof value === 'string' && /^#[\da-f]{6}$/i.test(value); }
  function effectName(value: LightingEffect | null | undefined) { return effects.find(item => item.value === value)?.label ?? '未知'; }
  function currentNumber(value: number | undefined, min: number, max: number, unit = '') { return value !== undefined && validInteger(value, min, max) ? value + unit : '未知'; }
  function effectDisabled(value: LightingEffect) { return locked || (value === 'static' ? !canStatic : !canDynamic); }
  $effect(() => {
    if (!dirty && !(draftEffect === 'static' && staticPending)) {
      draftEffect = effects.some(item => item.value === config?.effect) ? config!.effect : null;
      draftSpeed = config && validInteger(config.speed, 1, 9) ? config.speed : 5;
      draftBrightness = config && validInteger(config.brightness, 0, 100) ? config.brightness : 50;
      draftDirection = config?.direction === 2 ? 2 : 1;
      draftColor = validColor(config?.color) ? config!.color! : '#ffffff';
    }
  });
  $effect(() => { if (dirty && matchesReadback) dirty = false; });
  function markDraft() { dirty = true; submitted = false; error = ''; note = ''; }
  function chooseEffect(value: LightingEffect) {
    if (effectDisabled(value) || !effects.some(item => item.value === value)) return;
    if (draftEffect === value) return;
    draftEffect = value;
    markDraft();
  }
  function editNumber(field: 'speed' | 'brightness', event: Event & { currentTarget: EventTarget & HTMLInputElement }) {
    if (locked || !canDynamic) return;
    const value = event.currentTarget.valueAsNumber;
    if (field === 'speed') draftSpeed = Number.isFinite(value) ? value : null;
    else draftBrightness = Number.isFinite(value) ? value : null;
    markDraft();
  }
  function chooseDirection(value: string) {
    if (locked || !canDynamic || (value !== '1' && value !== '2')) return;
    draftDirection = value === '2' ? 2 : 1;
    markDraft();
  }
  function chooseColor(value: string) {
    if (locked || !canDynamic || ignoresColor || !validColor(value)) return;
    draftColor = value;
    markDraft();
  }
  async function applyAnimation() {
    if (locked || writeBlocked('lighting') || !canDynamic || !dynamicValid || !draftEffect || draftEffect === 'static') return;
    const request: LightingAnimation = { effect: draftEffect, speed: draftSpeed!, brightness: draftBrightness!, direction: draftDirection, color: draftColor };
    submitting = true; error = ''; note = ''; submitted = false;
    try { await onanimation(request); if (!disposed) submitted = true; }
    catch (cause) { if (!disposed) error = cause instanceof Error ? cause.message : typeof cause === 'string' ? cause : '动态灯效请求失败。'; }
    finally { if (!disposed) submitting = false; }
  }
  async function restore() {
    if (locked || writeBlocked('lighting') || !canRestore) return;
    restoring = true; error = ''; note = ''; submitted = false;
    try {
      await onrestore();
      if (!disposed) { dirty = false; note = '恢复请求已返回，请以当前设备状态为准。'; }
    } catch (cause) { if (!disposed) error = cause instanceof Error ? cause.message : typeof cause === 'string' ? cause : '灯光恢复请求失败。'; }
    finally { if (!disposed) restoring = false; }
  }
  onDestroy(() => { disposed = true; });
</script>

<div class="sh"><span>键盘灯效</span><StatusTag text={tag.text} kind={tag.kind}/></div>
<section class="group lighting-effects" aria-label="键盘灯效" aria-busy={submitting || restoring}>
  <div class="row">
    <span class="row-l" aria-live="polite">
      <b>当前灯效</b>
      {#if config}
        <small class="actual">{effectName(config.effect)} · 亮度值 {currentNumber(config.brightness, 0, 100)}{#if config.effect !== 'static'} · 速度 {currentNumber(config.speed, 1, 9)}{/if}{#if config.effect === 'wave' || config.effect === 'shifting'} · 方向 {config.direction === 1 || config.direction === 2 ? config.direction : '未知'}{/if}{#if config.effect !== 'static' && config.effect !== 'neon' && config.effect !== 'wave'} · 颜色 {validColor(config.color) ? config.color.toUpperCase() : '未知'}{/if}</small>
      {:else}
        <small>尚未读取到当前灯效。选择效果后编辑草稿，应用前不会修改键盘。</small>
      {/if}
    </span>
  </div>
  <ReadConflict readKey={config ? JSON.stringify(config) : null} edited={dirty || staticPending} matches={matchesReadback} label={effectName(config?.effect) + (config ? ` · 亮度 ${currentNumber(config.brightness, 0, 100)}` : '')} onadopt={() => { dirty = false; draftEffect = config?.effect ?? null; adoptRevision++; }}/>
  <div class="effects" role="group" aria-label="选择效果">
    {#each effects as item (item.value)}
      <button class="fx" type="button" aria-pressed={draftEffect === item.value} disabled={effectDisabled(item.value)} onclick={() => chooseEffect(item.value)}>
        <span class="sw-prev {item.preview}" aria-hidden="true"></span>
        <span>{item.label}</span>
        {#if config?.effect === item.value}<span class="cur">当前</span>{/if}
      </button>
    {/each}
  </div>
  <Collapse open={draftEffect !== null && draftEffect !== 'static'}>
    <div class="dynamic">
      <div class="slider-row">
        <label for={uid + '-speed'}>速度</label>
        <input class="slider" id={uid + '-speed'} type="range" min="1" max="9" step="1" style:--p={((validInteger(draftSpeed, 1, 9) ? draftSpeed : 5) - 1) / 8 * 100 + '%'} value={validInteger(draftSpeed, 1, 9) ? draftSpeed : 5} disabled={locked || !canDynamic} oninput={event => editNumber('speed', event)}/>
        <output for={uid + '-speed'}>{draftSpeed ?? '—'}</output>
      </div>
      <div class="slider-row">
        <label for={uid + '-brightness'}>亮度</label>
        <input class="slider" id={uid + '-brightness'} type="range" min="0" max="100" step="1" style:--p={(validInteger(draftBrightness, 0, 100) ? draftBrightness : 50) + '%'} value={validInteger(draftBrightness, 0, 100) ? draftBrightness : 50} disabled={locked || !canDynamic} oninput={event => editNumber('brightness', event)}/>
        <output for={uid + '-brightness'}>{draftBrightness ?? '—'}/100</output>
      </div>
      {#if hasDirection}
        <div class="row">
          <span class="row-l"><b>方向</b></span>
          <SegmentedControl size="small" value={String(draftDirection)} options={[{ value: '1', label: '方向 1' }, { value: '2', label: '方向 2' }]} onchange={chooseDirection} label="灯效方向" disabled={locked || !canDynamic}/>
        </div>
      {/if}
      <div class="row">
        <span class="row-l"><b>单色</b><small>{ignoresColor ? '此效果由设备生成配色，不使用单色设置。' : `选择灯效使用的颜色 · ${draftColor.toUpperCase()}`}</small></span>
        <div class="colors" role="group" aria-label="单色">
          {#each palette as color}
            <button class="cdot" type="button" style:background={color} aria-label={'颜色 ' + color.toUpperCase()} aria-pressed={!ignoresColor && draftColor.toLowerCase() === color} disabled={locked || !canDynamic || ignoresColor} onclick={() => chooseColor(color)}></button>
          {/each}
          <input class="cdot custom" class:selected={!ignoresColor && !paletteChoice} type="color" aria-label="自定义单色" value={validColor(draftColor) ? draftColor : '#ffffff'} disabled={locked || !canDynamic || ignoresColor} oninput={event => chooseColor(event.currentTarget.value)}/>
        </div>
      </div>
      <div class="row foot">
        <span class="hint">{draftPending ? `待应用：${effectName(draftEffect)} · 速度 ${draftSpeed ?? '—'} · 亮度 ${draftBrightness ?? '—'}` : '与当前设置一致。'} · 速度1–9，亮度0–100。{!dynamicValid ? '请输入有效整数与颜色。' : ''}</span>
        <button class="btn primary" type="button" disabled={locked || writeBlocked('lighting') || !canDynamic || !dynamicValid} onclick={applyAnimation}>{#if submitting}<span class="spinner" aria-hidden="true"></span>应用中…{:else}应用动态灯效{/if}</button>
      </div>
    </div>
  </Collapse>
  <Collapse open={draftEffect === 'static'}>
    <LightingControls zones={config?.effect === 'static' ? config.zones : null} brightness={config?.effect === 'static' ? config.brightness : null} canControl={canStatic} busy={locked} onapply={onstatic} resetRevision={adoptRevision} bind:pending={staticPending}/>
  </Collapse>
  {#if error}<div class="row msg"><p class="hint err-text" role="alert">{error}</p></div>{:else if submitted}<div class="row msg"><p class="hint" role="status">{matchesReadback ? '当前设备读回与灯效草稿一致。' : '请求已返回，尚未读回所选灯效，请查看设备状态与操作记录。'}</p></div>{:else if note}<div class="row msg"><p class="hint" role="status">{note}</p></div>{/if}
</section>

<div class="sh"><span>恢复</span></div>
<section class="group" aria-label="恢复上一步灯光">
  <div class="row">
    <span class="row-l"><b>恢复上一步</b><small>恢复设备读取组件保存的上一步真实灯光设置。</small>{#if !canRestore}<small>{restoreReason || '当前没有可用的灯光恢复状态。'}</small>{/if}</span>
    <button type="button" class="btn" disabled={locked || writeBlocked('lighting') || !canRestore} onclick={restore}>{#if restoring}<span class="spinner" aria-hidden="true"></span>恢复中…{:else}恢复上一步灯光{/if}</button>
  </div>
</section>

<style>
  .lighting-effects { min-width:0; }
  .effects { display:grid; grid-template-columns:repeat(4,minmax(0,1fr)); gap:12px; padding:16px 18px; }
  .fx { position:relative; display:flex; flex-direction:column; gap:7px; min-width:0; padding:0; border:0; background:none; text-align:left; font-size:12px; color:var(--label-2); border-radius:10px; }
  .fx > span:nth-child(2) { overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
  .fx .sw-prev { height:52px; border-radius:10px; box-shadow:inset 0 0 0 .5px rgba(0,0,0,.08); transition:box-shadow var(--d-base) var(--ease-out),transform var(--d-snappy) var(--spring-snappy); }
  .fx:hover:not(:disabled) .sw-prev { transform:translateY(-1px); }
  .fx:active:not(:disabled) .sw-prev { transform:scale(.97); }
  .fx[aria-pressed="true"] { color:var(--label); font-weight:500; }
  .fx[aria-pressed="true"] .sw-prev { box-shadow:0 0 0 2px var(--surface),0 0 0 4px var(--accent); }
  .fx .cur { position:absolute; top:6px; right:6px; font-size:10px; padding:1px 6px; border-radius:8px; background:rgba(0,0,0,.45); color:#fff; backdrop-filter:blur(6px); animation:cur-in var(--d-base) var(--ease-out); }
  @keyframes cur-in { from { opacity:0; } }
  .fx:disabled { opacity:.4; cursor:default; }
  .p-static { background:linear-gradient(90deg,#0a84ff 0 25%,#5e5ce6 25% 50%,#ff375f 50% 75%,#ff9f0a 75%); }
  .p-breath { background:radial-gradient(circle at 50% 50%,#ff375f,#2a0b14 75%); }
  .p-neon { background:conic-gradient(from 210deg,#ff375f,#ff9f0a,#ffd60a,#30d158,#64d2ff,#5e5ce6,#ff375f); }
  .p-wave { background:linear-gradient(100deg,#64d2ff,#5e5ce6 35%,#bf5af2 55%,#ff375f 80%,#ff9f0a); }
  .p-shift { background:linear-gradient(90deg,#30d158,#64d2ff); }
  .p-zoom { background:repeating-radial-gradient(circle at 50% 50%,#bf5af2 0 6px,#2b1440 6px 12px); }
  .p-meteor { background:linear-gradient(120deg,#0b0b1a 40%,#fff 48%,#64d2ff 52%,#0b0b1a 62%); }
  .p-flash { background:radial-gradient(circle,#ffd60a 1.5px,transparent 2px) 0 0/10px 10px,#1b1b24; }
  .dynamic { padding-top:4px; }
  .slider-row { display:grid; grid-template-columns:72px minmax(64px,1fr) 52px; align-items:center; gap:14px; padding:10px 18px; }
  .slider-row label { font-size:13px; font-weight:500; }
  .slider-row output { text-align:right; font-variant-numeric:tabular-nums; color:var(--label-2); }
  .colors { display:flex; flex-wrap:wrap; justify-content:flex-end; gap:10px; align-items:center; }
  .cdot { width:22px; height:22px; border-radius:50%; border:0; padding:0; cursor:pointer; box-shadow:inset 0 0 0 .5px rgba(0,0,0,.15); transition:box-shadow var(--d-base) var(--ease-out),transform var(--d-snappy) var(--spring-bouncy); }
  .cdot:active:not(:disabled) { transform:scale(.88); }
  .cdot[aria-pressed="true"],.cdot.selected { box-shadow:inset 0 0 0 .5px rgba(0,0,0,.15),0 0 0 2px var(--surface),0 0 0 4px var(--accent); }
  .cdot:disabled { opacity:.3; cursor:default; }
  .cdot.custom { -webkit-appearance:none; appearance:none; background:conic-gradient(#ff3b30,#ffcc00,#34c759,#0a84ff,#5e5ce6,#ff3b30); }
  .cdot.custom::-webkit-color-swatch-wrapper { padding:4px; }
  .cdot.custom::-webkit-color-swatch { border:0; border-radius:50%; }
  .cdot.custom::-moz-color-swatch { border:0; border-radius:50%; }
  .msg { min-height:0; padding-top:10px; padding-bottom:10px; }
  .msg p { margin:0; overflow-wrap:anywhere; }
  @media (max-width:900px) { .effects { grid-template-columns:repeat(2,minmax(0,1fr)); } }
  @media (prefers-reduced-motion:reduce) { .fx .sw-prev,.cdot { transition:none; } .fx:hover .sw-prev,.fx:active .sw-prev,.cdot:active { transform:none!important; } }
</style>
