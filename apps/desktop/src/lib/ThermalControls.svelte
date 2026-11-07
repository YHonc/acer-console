<script lang="ts">
  import { onMount } from 'svelte';
  import { cubicIn } from 'svelte/easing';
  import { api, type ThermalConfig, type ThermalStatus } from './api';
  import ConfirmDialog from './ui/ConfirmDialog.svelte';
  import SegmentedControl from './ui/SegmentedControl.svelte';
  import StatusTag from './ui/StatusTag.svelte';
  import { beginWrite, writeBlocked, anyWriting, isWriting } from './writeResources.svelte';
  let { allowed = false, onownership }: { allowed?: boolean; onownership?: (owned: boolean) => void } = $props();
  type Fan = 'cpu'|'gpu';
  type DraftPoint = { celsius: number|undefined; percent: number|undefined };
  type Draft = { points: DraftPoint[]; hysteresis_celsius: number; minimum_write_interval_ms: number; maximum_sample_age_ms: number };
  const blank = (): Draft => ({ points: [{celsius:undefined,percent:undefined},{celsius:undefined,percent:undefined}], hysteresis_celsius:3,minimum_write_interval_ms:1000,maximum_sample_age_ms:5000 });
  const uid = $props.id();
  let draft = $state<{cpu:Draft;gpu:Draft}>({cpu:blank(),gpu:blank()});
  let status = $state<ThermalStatus|null>(null);
  let statusKnown = $state(false), uncertain = $state(false);
  let busy = $state(false), loading = $state(true), error = $state(''), note = $state('');
  let confirmation = $state<ThermalConfig|null>(null);
  let editing = $state<Fan>('cpu');
  let nextId = 0;
  const makeIds = (count:number) => Array.from({length:count}, () => ++nextId);
  let ids = $state<Record<Fan, number[]>>({cpu:makeIds(2), gpu:makeIds(2)});
  let loadVersion = $state(0), enterId = $state(0);
  let disposed = false, reading = false;
  let stateVersion = 0;
  const owned = $derived(!!status?.active || !!status?.restore_pending);
  const resourceLocked = $derived(writeBlocked('performance'));
  const editLocked = $derived(loading||busy||isWriting('performance')||owned||uncertain);
  function config(): ThermalConfig {
    for (const key of ['cpu','gpu'] as const) {
      const curve=draft[key];
      if (curve.points.length<2 || curve.points.length>16) throw new Error('每条曲线需要 2 至 16 个点');
      curve.points.forEach((point,i)=>{
        if (!Number.isFinite(point.celsius) || point.celsius!<1 || point.celsius!>125 || !Number.isInteger(point.percent) || point.percent!<0 || point.percent!>100) throw new Error('请填写温度与风扇百分比，范围为 1–125°C、0–100%');
        if (i && (point.celsius!<=curve.points[i-1].celsius! || point.percent!<curve.points[i-1].percent!)) throw new Error('温度必须递增，风扇百分比不能随温度升高而降低');
      });
      if (!Number.isFinite(curve.hysteresis_celsius) || curve.hysteresis_celsius<0 || curve.hysteresis_celsius>10) throw new Error('降速滞回应为 0–10°C');
    }
    return structuredClone($state.snapshot(draft)) as ThermalConfig;
  }
  const validation = $derived.by(()=>{try{config();return '';}catch(e){return String(e instanceof Error?e.message:e);}});
  const tag = $derived<{text:string;kind:'ok'|'draft'|'er'|''}>(
    !statusKnown ? {text:'状态未确认',kind:''}
    : status?.restore_pending ? {text:'恢复待确认',kind:'er'}
    : status?.active ? {text:'运行中',kind:'ok'}
    : {text:'已停止',kind:''});
  function accept(value:ThermalStatus) { status=value;statusKnown=true;uncertain=false; onownership?.(value.active||value.restore_pending); }
  async function refresh() {
    if (reading || busy || disposed) return;
    reading=true;const version=stateVersion;
    try { const value=await api.getThermalStatus();if(!disposed&&version===stateVersion)accept(value); }
    catch(e){if(!disposed&&version===stateVersion){error=String(e);statusKnown=false;}}
    finally{reading=false;}
  }
  onMount(()=>{
    disposed=false;
    void (async()=>{
      try{
        const saved=await api.getThermalConfig();
        if(!disposed&&saved){draft=saved;ids={cpu:makeIds(saved.cpu.points.length),gpu:makeIds(saved.gpu.points.length)};loadVersion++;}
      }
      catch(e){if(!disposed)error=String(e);}
      finally{if(!disposed)loading=false;}
      await refresh();
    })();
    const timer=setInterval(()=>{if(!document.hidden&&!anyWriting())void refresh();},5000);
    return ()=>{disposed=true;clearInterval(timer);};
  });
  async function save() {
    if (loading || busy || validation) return;
    const release=beginWrite('performance');if(!release)return;
    busy=true;error='';note='';
    try{await api.saveThermalConfig(config());if(!disposed)note='曲线已保存；不会自动启动。';}catch(e){if(!disposed)error=String(e);}finally{release();if(!disposed)busy=false;}
  }
  async function start() {
    if (loading||busy||owned||uncertain||!allowed||validation||!statusKnown)return;
    const release=beginWrite('performance');if(!release)return;
    const target=confirmation;if(!target){release();return;}++stateVersion;confirmation=null;busy=true;error='';note='';uncertain=true;statusKnown=false;onownership?.(true);
    try{const value=await api.startThermal(target);if(!disposed)accept(value);}catch(e){if(!disposed)error=String(e);}finally{release();if(!disposed)busy=false;}
    await refresh();
  }
  async function stop() {
    if(busy||(!owned&&!uncertain&&!allowed))return;
    const release=beginWrite('performance');if(!release)return;
    ++stateVersion;busy=true;error='';note='';uncertain=true;statusKnown=false;onownership?.(true);
    try{const value=await api.stopThermal();if(!disposed)accept(value);}catch(e){if(!disposed)error=String(e);}finally{release();if(!disposed)busy=false;}
    await refresh();
  }
  function addPoint() {
    if (editLocked || draft[editing].points.length>=16) return;
    const id=++nextId;
    draft[editing].points.push({celsius:undefined,percent:undefined});
    ids[editing].push(id);
    enterId=id;
  }
  function removePoint(index:number) {
    if (editLocked || draft[editing].points.length<=2) return;
    draft[editing].points.splice(index,1);
    ids[editing].splice(index,1);
  }

  const X = (celsius:number) => 40 + celsius/125*500;
  const Y = (percent:number) => 186 - percent/100*166;
  const xTicks = [0,25,50,75,100,125], yTicks = [0,25,50,75,100];
  const drawOrder: Fan[] = ['gpu','cpu'];
  function validCelsius(value:number|undefined) { return Number.isFinite(value) && value!>=1 && value!<=125; }
  function validPercent(value:number|undefined) { return Number.isInteger(value) && value!>=0 && value!<=100; }
  function filled(value:number|undefined) { return value!==undefined && value!==null; }
  function plotted(fan:Fan) {
    return draft[fan].points
      .map((point,index)=>({id:ids[fan][index] ?? -index-1, celsius:point.celsius, percent:point.percent}))
      .filter((point):point is {id:number;celsius:number;percent:number}=>validCelsius(point.celsius)&&validPercent(point.percent))
      .map(point=>({id:point.id, x:X(point.celsius), y:Y(point.percent)}));
  }
  const series = $derived({cpu:plotted('cpu'), gpu:plotted('gpu')});
  function line(points:{x:number;y:number}[]) { return points.length ? 'M'+points.map(p=>`${p.x.toFixed(1)} ${p.y.toFixed(1)}`).join(' L') : ''; }
  function area(points:{x:number;y:number}[]) { return points.length ? `${line(points)} L${points.at(-1)!.x.toFixed(1)} 186 L${points[0].x.toFixed(1)} 186 Z` : ''; }
  const cssPath = (value:string) => `path("${value}")`;
  const reduceMotion = () => typeof matchMedia === 'function' && matchMedia('(prefers-reduced-motion: reduce)').matches;
  function rowOut(_node:Element) {
    const reduce=reduceMotion();
    return { duration:140, easing:cubicIn, css:(t:number)=>reduce?`opacity:${t}`:`opacity:${t};transform:translateX(${((1-t)*-8).toFixed(2)}px)` };
  }
</script>

<div class="sh"><span>自定义温控曲线</span><StatusTag text={tag.text} kind={tag.kind}/></div>
<section class="group thermal-section" aria-label="自定义温控曲线" aria-busy={busy||loading}>
  <div class="pad intro">
    <b>随温度调节双风扇</b>
    <p class="hint">软件运行时按曲线调节，停止后恢复自动模式。当前仅支持接电运行，尚待本机完整实测。</p>
    <p class="hint">下方范围是参数表达范围，不是推荐安全范围。请自行填写曲线；未提供未经验证的默认温控点。</p>
  </div>
  <div class="pad chart-pad">
    <div class="chart-head">
      <div class="legend" aria-hidden="true"><span><i class="cpu"></i>CPU</span><span><i class="gpu"></i>GPU</span></div>
      <div class="curve-switch">
        <SegmentedControl size="small" value={editing} options={[{value:'cpu',label:'编辑 CPU'},{value:'gpu',label:'编辑 GPU'}]} onchange={(value)=>{if(value==='cpu'||value==='gpu'){editing=value;enterId=0;}}} label="编辑哪条曲线"/>
      </div>
    </div>
    <svg class="chart" viewBox="0 0 560 210" role="img" aria-label="温控曲线预览，横轴温度，纵轴风扇百分比">
      <defs>
        <linearGradient id={uid+'-ga'} x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="var(--accent)" stop-opacity=".18"/><stop offset="1" stop-color="var(--accent)" stop-opacity="0"/></linearGradient>
        <linearGradient id={uid+'-gb'} x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="var(--indigo)" stop-opacity=".18"/><stop offset="1" stop-color="var(--indigo)" stop-opacity="0"/></linearGradient>
      </defs>
      {#each yTicks as tick}<line class="grid" x1="40" x2="540" y1={Y(tick)} y2={Y(tick)}/><text x="32" y={Y(tick)+3} text-anchor="end">{tick}%</text>{/each}
      {#each xTicks as tick}<text x={X(tick)} y="203" text-anchor="middle">{tick}°C</text>{/each}
      {#each drawOrder as fan (fan)}
        {@const path=line(series[fan])}
        {@const fillPath=area(series[fan])}
        {#if fillPath}<path class="area" class:hidden={editing!==fan} d={fillPath} style:d={cssPath(fillPath)} fill={`url(#${uid}-${fan==='cpu'?'ga':'gb'})`}/>{/if}
        {#if path}<path class="curve {fan}" class:dim={editing!==fan} d={path} style:d={cssPath(path)}/>{/if}
      {/each}
      <g class="pts {editing}">
        {#each series[editing] as point (point.id)}
          <g class="pt" style:transform={`translate(${point.x}px, ${point.y}px)`}><circle class:enter={point.id===enterId} r="5" cx="0" cy="0"/></g>
        {/each}
      </g>
      {#if !series.cpu.length && !series.gpu.length}<text class="empty" x="290" y="106" text-anchor="middle">尚未填写有效温控点</text>{/if}
    </svg>
  </div>
  <fieldset class="editor" disabled={editLocked}>
    <legend class="sr-only">{editing.toUpperCase()} 温度与风扇目标</legend>
    {#key `${editing}:${loadVersion}`}
      <div class="pt-list">
        {#each draft[editing].points as point,i (ids[editing][i] ?? `missing-${i}`)}
          <div class="pt-row" class:enter={ids[editing][i]===enterId} out:rowOut>
            <span>点 {i+1}</span>
            <label>温度<input class="num" class:invalid={filled(point.celsius)&&!validCelsius(point.celsius)} type="number" min="1" max="125" step="1" aria-label={`${editing} 第 ${i+1} 点温度`} bind:value={point.celsius}/>°C</label>
            <label>风扇<input class="num" class:invalid={filled(point.percent)&&!validPercent(point.percent)} type="number" min="0" max="100" step="1" aria-label={`${editing} 第 ${i+1} 点百分比`} bind:value={point.percent}/>%</label>
            <button class="round-x" type="button" disabled={draft[editing].points.length<=2} aria-label={`删除 ${editing} 第 ${i+1} 点`} onclick={()=>removePoint(i)}><svg viewBox="0 0 12 12" aria-hidden="true"><path d="M3 6h6"/></svg></button>
          </div>
        {/each}
      </div>
    {/key}
    <div class="row options">
      <button class="btn plain" type="button" disabled={draft[editing].points.length>=16} onclick={addPoint}>＋ 添加曲线点</button>
      <label class="hyst"><span class="hint">{editing.toUpperCase()} 降速滞回</span><input class="num" class:invalid={!Number.isFinite(draft[editing].hysteresis_celsius)||draft[editing].hysteresis_celsius<0||draft[editing].hysteresis_celsius>10} type="number" min="0" max="10" step="0.5" bind:value={draft[editing].hysteresis_celsius}/><span class="hint">°C</span></label>
    </div>
  </fieldset>
  {#if (statusKnown && status?.message) || uncertain || error || note}
    <div class="pad messages">
      {#if statusKnown && status?.message}<p class="hint">{status.message}</p>{/if}
      {#if uncertain}<p class="hint warn" role="status">请求结果未确认。请查询状态或重试停止，不能据此认为风扇已恢复。</p>{/if}
      {#if error}<p class="hint err-text" role="alert">{error}</p>{/if}
      {#if note}<p class="hint" role="status">{note}</p>{/if}
    </div>
  {/if}
  <div class="row foot">
    <span class="hint" class:err-text={!!validation} aria-live="polite">{validation || `CPU 目标 ${status?.cpu_target ?? '—'}% · GPU 目标 ${status?.gpu_target ?? '—'}%`}</span>
    <span class="actions">
      <button class="btn" type="button" disabled={loading||busy||resourceLocked||!!validation} onclick={save}>保存曲线</button>
      <button class="btn primary" type="button" disabled={loading||busy||resourceLocked||owned||uncertain||!allowed||!!validation||!statusKnown} onclick={()=>{try{confirmation=config();}catch(e){error=String(e);}}}>启动曲线</button>
      <button class="btn" type="button" disabled={busy||resourceLocked||(!owned&&!uncertain&&!allowed)} onclick={stop}>{status?.restore_pending||uncertain?'重试恢复自动':'停止并恢复自动'}</button>
    </span>
  </div>
</section>
{#if confirmation}
  <ConfirmDialog title="启动自定义温控曲线" oncancel={()=>confirmation=null}>
    <p>将使用当前填写的 CPU/GPU 曲线持续控制风扇。官方控制台可能争用控制权；检测到外部改写、温度失效或断电时会停止并尝试恢复自动模式。</p>
    <p>这不是固件曲线。程序或系统被强制终止时，无法保证恢复执行。首次使用请观察温度与转速。</p>
    <div class="action-row"><button class="btn" type="button" data-dialog-cancel onclick={()=>confirmation=null}>取消</button><button class="btn primary" type="button" onclick={start}>确认启动</button></div>
  </ConfirmDialog>
{/if}

<style>
  .thermal-section{min-width:0}
  .intro b{font-size:15px;font-weight:600}
  .intro p{margin:6px 0 0;overflow-wrap:anywhere}
  .chart-pad{padding-top:4px}
  .chart-head{display:flex;align-items:center;justify-content:space-between;gap:12px;flex-wrap:wrap;margin-bottom:8px}
  .legend{display:flex;gap:16px;align-items:center;font-size:12px;color:var(--label-2)}
  .legend i{display:inline-block;width:14px;height:3px;border-radius:2px;margin-right:6px;vertical-align:middle}
  .legend i.cpu{background:var(--accent)}
  .legend i.gpu{background:var(--indigo)}
  .curve-switch{flex:none}
  .chart{display:block;width:100%;height:auto;overflow:visible}
  .chart .grid{stroke:var(--sep);stroke-width:1}
  .chart text{fill:var(--label-3);font-size:10px;font-family:var(--font)}
  .chart text.empty{fill:var(--label-2);font-size:12px}
  .chart .curve{fill:none;stroke-width:2.25;stroke-linejoin:round;stroke-linecap:round;transition:d var(--d-smooth) var(--spring-smooth),opacity var(--d-base)}
  .chart .area{transition:d var(--d-smooth) var(--spring-smooth),opacity var(--d-base)}
  .chart .area.hidden{opacity:0}
  .chart .curve.cpu{stroke:var(--accent)}
  .chart .curve.gpu{stroke:var(--indigo)}
  .chart .dim{opacity:.28}
  .chart .pt{transition:transform var(--d-smooth) var(--spring-smooth)}
  .chart .pt circle{stroke:var(--surface);stroke-width:2.5;transform-box:fill-box;transform-origin:center}
  .chart .pts.cpu circle{fill:var(--accent)}
  .chart .pts.gpu circle{fill:var(--indigo)}
  .chart .pt circle.enter{animation:pt-pop var(--d-bouncy) var(--spring-bouncy) both}
  @keyframes pt-pop{from{transform:scale(0)}}
  .editor{margin:0;padding:0;border:0;min-width:0}
  .editor:disabled{opacity:.5}
  .sr-only{position:absolute;width:1px;height:1px;padding:0;margin:-1px;overflow:hidden;clip:rect(0,0,0,0);white-space:nowrap;border:0}
  .pt-list{padding:2px 0 8px}
  .pt-row{display:grid;grid-template-columns:48px auto auto 1fr;column-gap:28px;align-items:center;gap:12px;padding:6px 18px}
  .pt-row span{font-size:12px;color:var(--label-2)}
  .pt-row label{display:flex;align-items:center;gap:8px;font-size:12px;color:var(--label-2)}
  .pt-row.enter{animation:row-fade 240ms var(--ease-out) both,row-rise var(--d-snappy) var(--spring-snappy) both}
  @keyframes row-fade{from{opacity:0}}
  @keyframes row-rise{from{transform:translateY(12px)}}
  .round-x{justify-self:end;width:26px;height:26px;border-radius:50%;border:0;padding:0;background:var(--fill-2);color:var(--label-2);display:grid;place-items:center;transition:background-color var(--d-fast),color var(--d-fast),transform var(--d-snappy) var(--spring-snappy)}
  .round-x:hover:not(:disabled){background:var(--red-soft);color:var(--red-text)}
  .round-x:active:not(:disabled){transform:scale(.9)}
  .round-x:disabled{opacity:.3;cursor:default}
  .round-x svg{width:12px;height:12px;stroke:currentColor;stroke-width:2;fill:none;stroke-linecap:round}
  .options{gap:10px}
  .hyst{display:flex;align-items:center;gap:8px;margin-left:auto}
  .messages{display:grid;gap:4px}
  .messages p{margin:0;overflow-wrap:anywhere}
  .warn{color:var(--orange-text)}
  .foot{flex-wrap:wrap}
  .actions{display:flex;gap:8px;flex-wrap:wrap}
  @media (max-width:560px){.pt-row{grid-template-columns:40px 1fr 1fr auto;column-gap:10px}}
  @media (prefers-reduced-motion:reduce){
    .chart .curve,.chart .area,.chart .pt,.round-x{transition:none}
    .chart .pt circle.enter{animation:none}
    .pt-row.enter{animation:row-fade 140ms linear both}
  }
</style>
