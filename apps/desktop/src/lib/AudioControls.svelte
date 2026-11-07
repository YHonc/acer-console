<script lang="ts">
  import PopupSelect from './ui/PopupSelect.svelte';
  // No authorization means no query. Never seed a device preset or synthesize an EQ editor.
  import { onDestroy, untrack } from 'svelte';
  import { api, errorText, type AudioPreset, type AudioState } from './api';
  import StatusTag from './ui/StatusTag.svelte';
  import ReadConflict from './ui/ReadConflict.svelte';
  import { beginWrite, writeBlocked } from './writeResources.svelte';
  let { authorized, refreshToken = 0 }: { authorized: boolean; refreshToken?: number } = $props();
  const uid = $props.id();
  const labels: Record<AudioPreset, string> = {
    music: '音乐', movie: '电影', voice: '语音', strategy: '策略游戏',
    rpg: '角色扮演', shooter: '射击游戏', custom: '自定义', automatic: '自动',
  };
  let readback = $state<AudioState | null>(null);
  let draftPreset = $state<AudioPreset | null>(null);
  let draftEdited = $state(false);
  let reading = $state(false);
  let writing = $state(false);
  let fresh = $state(false);
  let error = $state('');
  let result = $state<{ confirmed: boolean; message: string } | null>(null);
  let disposed = false;
  let refreshQueued = false;
  let requestSerial = 0;
  const supported = $derived([...new Set(readback?.supported.filter(isPreset) ?? [])]);
  let readPromise: Promise<void> | null = null;
  const locked = $derived(writeBlocked('audio'));
  const editable = $derived(authorized && fresh && !locked && readback?.availability === 'available' && readback.writable === true && !readback.error && isPreset(readback.preset) && supported.length > 0);
  const draftValid = $derived(draftPreset !== null && supported.includes(draftPreset));
  const dirty = $derived(draftPreset !== null && draftPreset !== readback?.preset);
  const tag = $derived<{ text: string; kind: 'ok' | 'draft' | 'er' | '' }>(
    !authorized ? { text: '等待授权', kind: '' }
    : writing ? { text: '应用中', kind: 'draft' }
    : reading ? { text: '读取中', kind: '' }
    : !fresh ? { text: '状态未知', kind: error ? 'er' : '' }
    : dirty ? { text: '待应用草稿', kind: 'draft' }
    : { text: '与设备读回一致', kind: 'ok' }
  );

  function isPreset(value: unknown): value is AudioPreset { return typeof value === 'string' && Object.hasOwn(labels, value); }
  function presetName(value: unknown) { return isPreset(value) ? labels[value] : '未知'; }
  function validate(value: AudioState) {
    if (!value || value.backend !== 'official_service' || !Array.isArray(value.supported) || !['available', 'output_unavailable', 'unknown'].includes(value.availability)) throw new Error('音效状态返回不完整，当前预设未知。');
    return value;
  }
  function readable(value: AudioState) { return value.availability === 'available' && !value.error && isPreset(value.preset); }
  function acceptRead(value: AudioState) {
    readback = value;
    fresh = readable(value);
    if (fresh && isPreset(value.preset) && value.supported.includes(value.preset) && (!draftEdited || draftPreset === value.preset)) {
      draftPreset = value.preset;
      draftEdited = false;
    }
    if (value.availability !== 'output_unavailable' && value.error) error = value.error;
  }
  function activeRequest(serial: number) { return !disposed && authorized && serial === requestSerial; }
  function refresh(): Promise<void> {
    if (readPromise) return readPromise;
    if (disposed || !authorized) return Promise.resolve();
    if (writing) { refreshQueued = true; return Promise.resolve(); }
    const serial = ++requestSerial;
    reading = true; error = ''; result = null;
    readPromise = (async () => { try {
      const next = validate(await api.getAudio());
      if (activeRequest(serial)) acceptRead(next);
    } catch (cause) { if (activeRequest(serial)) { fresh = false; error = '音效状态读取失败：' + errorText(cause); } }
    finally {
      readPromise = null;
      if (!disposed) {
        reading = false;
        if (refreshQueued && authorized) { refreshQueued = false; void refresh(); }
      }
    }
    })();
    return readPromise;
  }
  function editPreset(event: Event & { currentTarget: EventTarget & HTMLSelectElement }) {
    if (!editable || !isPreset(event.currentTarget.value) || !supported.includes(event.currentTarget.value)) return;
    draftPreset = event.currentTarget.value;
    draftEdited = true; error = ''; result = null;
  }
  async function apply() {
    if (!editable || !dirty || !draftValid || draftPreset === null) return;
    const release = beginWrite('audio');
    if (!release) return;
    const target = draftPreset;
    const serial = ++requestSerial;
    writing = true; fresh = false; error = ''; result = null;
    try {
      const outcome = await api.setAudio(target);
      if (!activeRequest(serial)) return;
      if (outcome.status === 'failed') throw new Error(outcome.message || '音效预设请求失败。');
      const next = validate(await api.getAudio());
      if (!activeRequest(serial)) return;
      readback = next;
      const confirmed = outcome.status === 'applied' && readable(next) && next.preset === target;
      fresh = confirmed;
      if (confirmed) { draftPreset = next.preset; draftEdited = false; }
      result = {
        confirmed,
        message: confirmed ? '音效预设已读回确认：' + presetName(next.preset) + '。' : '音效设置结果未确认：' + (next.availability === 'output_unavailable' ? '当前音频输出不支持此音效，请切换输出设备后刷新。' : next.error || outcome.message || '设备读回与请求不一致，请刷新后检查。'),
      };
    } catch (cause) { if (activeRequest(serial)) error = '音效预设设置失败，状态已过期：' + errorText(cause); }
    finally {
      release();
      if (!disposed) {
        writing = false;
        if (refreshQueued && authorized) { refreshQueued = false; void refresh(); }
      }
    }
  }
  $effect(() => {
    const allowed = authorized;
    void refreshToken;
    untrack(() => {
      if (!allowed) {
        requestSerial += 1;
        fresh = false; error = ''; result = null; refreshQueued = false;
      } else void refresh();
    });
  });
  onDestroy(() => { disposed = true; requestSerial += 1; });
</script>

<div class="sh"><span>DTS 音效</span><span><StatusTag text={tag.text} kind={tag.kind}/></span></div>
<div class="group" role="group" aria-label="音效预设" aria-busy={authorized && (reading || writing)}>
  <div class="row">
    <span class="row-l" aria-live="polite"><b>{fresh ? '设备当前预设' : '上次 / 未确认'}</b><small>依赖现有 Acer 音效服务与 DTS 组件。</small></span>
    <span class="row-v">{presetName(readback?.preset)}</span>
    <button type="button" class="btn" disabled={!authorized} aria-busy={reading} onclick={refresh}>{#if reading && authorized}<span class="spinner" aria-hidden="true"></span>读取中…{:else}刷新状态{/if}</button>
  </div>
  <div class="row">
    <label class="row-l" for={uid + '-preset'}><b>选择音效</b><small>只编辑草稿，点击应用后再读取确认。</small>{#if draftPreset === 'custom' || readback?.preset === 'custom'}<small>自定义仅切换已有预设，此处不提供均衡器编辑。</small>{/if}</label>
    <PopupSelect id={uid + '-preset'} aria-label="DTS 音效" value={draftValid ? draftPreset! : ''} current={fresh ? readback?.preset : null} unknown={!fresh} disabled={!editable} onchange={editPreset} options={supported.map(preset=>({value:preset,label:labels[preset]}))}/>
  </div>
  {#if !authorized}<div class="row"><p class="hint" role="status">请先授权设备访问，再读取和修改音效。</p></div>
  {:else if error}<div class="row"><p class="hint er" role="alert">{error}</p></div>
  {:else if result}<div class="row"><p class="hint" class:ok-text={result.confirmed} role="status">{result.message}</p></div>
  {:else if readback?.availability === 'output_unavailable'}<div class="row"><p class="hint" role="status">当前音频输出不支持此音效。请切换到受支持的输出设备后刷新。</p></div>
  {:else if !reading && !fresh}<div class="row"><p class="hint" role="status">当前音效状态未知，请刷新后操作。</p></div>
  {:else if fresh && !supported.length}<div class="row"><p class="hint" role="status">当前没有原生接口确认的可选预设。</p></div>
  {:else if fresh && readback?.writable !== true}<div class="row"><p class="hint" role="status">当前音效预设仅可读取，暂不可修改。</p></div>{/if}
  <ReadConflict readKey={fresh ? readback?.preset ?? null : null} edited={draftEdited} matches={!dirty} label={presetName(readback?.preset)} onadopt={() => { draftPreset = readback?.preset ?? null; draftEdited = false; }}/>
  <div class="row foot">
    <span class="hint" aria-live="polite">{draftPreset ? !fresh ? '上次草稿（状态未确认）：' + presetName(draftPreset) : dirty ? '待应用：' + presetName(draftPreset) : '与设备读回一致' : '尚无预设草稿'}</span>
    <button type="button" class="btn primary" disabled={!editable || !draftValid || !dirty} onclick={apply}>{#if writing}<span class="spinner" aria-hidden="true"></span>应用中…{:else}应用音效{/if}</button>
  </div>
</div>

<style>
  p.hint {margin:0;overflow-wrap:anywhere;}
  .er {color:var(--red-text, var(--danger));}
  .ok-text {color:var(--green-text, var(--success));}
  label.row-l {cursor:pointer;}
  .row > .btn {flex-shrink:0;}
  @media(max-width:420px) {.row {flex-wrap:wrap;}}
</style>
