<script lang="ts">
  import PopupSelect from './ui/PopupSelect.svelte';
  import AnimatedNumber from './ui/AnimatedNumber.svelte';
  import ConfirmDialog from './ui/ConfirmDialog.svelte';
  import { onMount } from 'svelte';
  import { api, errorText, type CentralPluginState, type PluginLog, type PluginLogResult } from './api';
  import Disclosure from './ui/Disclosure.svelte';
  import Switch from './ui/Switch.svelte';
  import Collapse from './ui/Collapse.svelte';
  import StatusTag from './ui/StatusTag.svelte';
  import { beginWrite, writeBlocked, isWriting } from './writeResources.svelte';
  let { authorized=false, onauthorize }: {authorized?:boolean;onauthorize:()=>Promise<void>}=$props();
  let plugin=$state<CentralPluginState|null>(null);
  let pending=$state(false), problem=$state(''), feedback=$state('');
  let reading=$state(false);
  let readPromise:Promise<void>|null=null;
  const busy=$derived(writeBlocked('central'));
  let auto=$state(false), showLogs=$state(false), logPending=$state(false), logError=$state('');
  let logKind=$state<PluginLog>('events'), logResult=$state<PluginLogResult|null>(null);
  let confirmation=$state<'enable'|'disable'|null>(null);
  let samples=$state<{time:number;cpu:number;memory:number;handles:number;identity:string}[]>([]);
  let latestCpu=$state<number|null>(null);
  let disposed=false, serial=0, logSerial=0;
  const metrics=$derived(plugin?.metrics);
  const first=$derived(samples[0]);
  const last=$derived(samples.at(-1));
  const span=$derived(first && last ? (last.time-first.time)/1000 : 0);
  const averageCpu=$derived(span>0 && first && last && plugin ? (last.cpu-first.cpu)/span/Math.max(1,plugin.logical_processors)*100 : null);
  const handlesPerMinute=$derived(span>0 && first && last ? (last.handles-first.handles)/span*60 : null);
  const peakMemory=$derived(samples.length ? Math.max(...samples.map(s=>s.memory)) : null);
  const meanMemory=$derived(samples.length ? samples.reduce((sum,s)=>sum+s.memory,0)/samples.length : null);
  const serviceText=$derived(plugin?.service?.patch==='patched'?'运行中 · 补丁已识别':plugin?.service?.patch==='original'?'运行中 · 原厂版本':'尚未确认');
  const guardText=$derived(plugin?.guard.installed===false?'未安装':plugin?.guard.enabled===true?'自动检查已启用':plugin?.guard.enabled===false?'自动检查已暂停':'状态未知');
  function number(value:number|null|undefined,digits=0) {return value==null||!Number.isFinite(value)?'—':value.toLocaleString('zh-CN',{maximumFractionDigits:digits,minimumFractionDigits:digits});}
  function mib(value:number|null|undefined){return value==null?'—':number(value/1048576,2)+' MiB';}
  function date(value:string|null|undefined){return !value||value.startsWith('1899')?'—':new Date(value).toLocaleString();}
  function addSample(next:CentralPluginState){
    const m=next.metrics;if (!m) {latestCpu=null;return;}
    const identity=`${m.pid}:${m.started_at}`,previous=samples.at(-1);
    if(previous && (previous.identity!==identity || next.captured_at_ms<=previous.time || m.cpu_seconds<previous.cpu)){samples=[];latestCpu=null;}
    else if(previous){latestCpu=(m.cpu_seconds-previous.cpu)/((next.captured_at_ms-previous.time)/1000)/Math.max(1,next.logical_processors)*100;}
    samples=[...samples,{time:next.captured_at_ms,cpu:m.cpu_seconds,memory:m.working_set_bytes,handles:m.handles,identity}].slice(-120);
  }
  function refresh():Promise<void>{
    if(readPromise)return readPromise;if(disposed)return Promise.resolve();const request=++serial;reading=true;problem='';
    readPromise=(async()=>{
    try {const next=await api.getCentralPlugin();if(disposed||request!==serial||pending)return;plugin=next;addSample(next);}
    catch(e){if(!disposed&&request===serial&&!pending)problem=errorText(e);}
    finally{reading=false;readPromise=null;}
    })();return readPromise;
  }
  async function change(){
    const action=confirmation;if(!action||pending||busy||!authorized||!plugin?.guard.verified||problem||plugin.guard.enabled==null)return;
    const release=beginWrite('central');if(!release)return;++serial;
    confirmation=null;pending=true;problem='';feedback='';
    try {const next=await api.setCentralGuard(action);if(disposed)return;++serial;plugin=next;addSample(next);feedback=next.guard.enabled===(action==='enable')?'看门狗任务状态已读回确认。':'请求已发送，请刷新确认当前状态。';}
    catch(e){if(!disposed)problem=errorText(e);}
    finally{release();if(!disposed)pending=false;}
  }
  async function readLog(){
    const request=++logSerial;logPending=true;logError='';logResult=null;
    try{const result=await api.getCentralLog(logKind);if(!disposed&&request===logSerial&&showLogs)logResult=result;}
    catch(e){if(!disposed&&request===logSerial)logError=errorText(e);}
    finally{if(!disposed&&request===logSerial)logPending=false;}
  }
  function toggleLogs(){if(showLogs){void readLog();}else{logSerial++;logPending=false;logResult=null;logError='';}}
  onMount(()=>{void refresh();const timer=setInterval(()=>{if(auto&&!document.hidden&&!pending)void refresh();},15000);return()=>{disposed=true;serial++;logSerial++;clearInterval(timer);};});
</script>

{#snippet metric(value: number | null | undefined, digits = 0)}<AnimatedNumber {value} decimals={digits} minimumFractionDigits={digits}/>{/snippet}
{#snippet memoryMetric(value: number | null | undefined)}<AnimatedNumber value={value == null ? null : value / 1048576} decimals={2} minimumFractionDigits={2} suffix=" MiB"/>{/snippet}

{#snippet busyLabel(active: boolean, busyText: string, idleText: string)}{#if active}<span class="spinner" aria-hidden="true"></span>{busyText}{:else}{idleText}{/if}{/snippet}
{#snippet runtime()}{#if metrics}{@render metric(Math.max(0,(plugin!.captured_at_ms-Date.parse(metrics.started_at))/60000),1)} 分钟{:else}—{/if}{/snippet}

<div class="plugin-page">
  <div class="group">
    <div class="row">
      <span class="sq indigo" aria-hidden="true"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3 5 6v5.5c0 4.2 3 7.8 7 9.5 4-1.7 7-5.3 7-9.5V6z"/><path d="M9 12l2 2 4-4"/></svg></span>
      <span class="row-l"><b>Acer Central 句柄修补与 Handle Guard</b><small>每 15 秒自动刷新插件状态</small></span>
      <Switch bind:checked={auto} aria-label="每 15 秒刷新"/>
      <button type="button" class="btn" aria-busy={reading} onclick={refresh}>{@render busyLabel(reading, '读取中…', '刷新插件状态')}</button>
    </div>
    {#if problem}<div class="row"><p class="hint er" role="alert">{problem}{plugin?' · 下方保留上次读数，当前状态未确认。':''}</p></div>{/if}
    {#if feedback}<div class="row"><p class="hint" role="status">{feedback}</p></div>{/if}
  </div>

  <div class="sh"><span>Central 服务</span><span><StatusTag text={serviceText} kind={plugin?.service?.patch==='patched'?'ok':plugin?.service?.patch==='original'?'draft':''}/></span></div>
  <div class="group">
    <div class="row"><span class="row-l"><b>PID / 启动方式</b></span><span class="row-v">{plugin?.service?.pid||'—'} / {plugin?.service?.start_mode??'未知'}</span></div>
    <div class="row"><span class="row-l"><b>CPU 占用</b></span><span class="row-v">{@render metric(latestCpu,2)} %</span></div>
    <div class="row"><span class="row-l"><b>工作集 / 私有提交</b></span><span class="row-v">{@render memoryMetric(metrics?.working_set_bytes)} / {@render memoryMetric(metrics?.private_commit_bytes)}</span></div>
    <div class="row"><span class="row-l"><b>句柄 / 线程</b></span><span class="row-v">{@render metric(metrics?.handles)} / {@render metric(metrics?.threads)}</span></div>
    <div class="row"><span class="row-l"><b>运行时长</b></span><span class="row-v">{@render runtime()}</span></div>
    <div class="row"><span class="row-l"><b>采样</b></span><span class="row-v">{plugin?new Date(plugin.captured_at_ms).toLocaleTimeString():'等待采样'}</span></div>
    {#if plugin?.errors.length}<div class="row"><span class="row-l"><b>指标错误</b></span><span class="row-v er">{plugin.errors.join('；')} · 无权限的指标保持未知，不按零计算。</span></div>{/if}
    <div class="disc-row"><Disclosure title="程序路径与校验"><div class="pad detail mono"><p>{plugin?.service?.path??'未读取'}</p><p>{plugin?.service?.sha256??'未取得文件哈希'}</p><p class="hint">按服务配置路径校验文件哈希，不等同于验证进程内存映像；仍可能有慢速句柄积累。</p></div></Disclosure></div>
  </div>

  <div class="sh"><span>Handle Guard 看门狗</span><span><StatusTag text={guardText} kind={plugin?.guard.enabled===true?'ok':plugin?.guard.enabled===false?'draft':plugin?.guard.installed===false?'':plugin?.guard.error?'er':''}/></span></div>
  <div class="group">
    <div class="row"><span class="row-l"><b>句柄阈值 / 冷却</b></span><span class="row-v">{@render metric(plugin?.guard.threshold)} / {@render metric(plugin?plugin.guard.cooldown_seconds/60:null)} 分钟</span></div>
    <div class="row"><span class="row-l"><b>任务状态 / 返回码</b></span><span class="row-v">{plugin?.guard.state===4?'正在运行':plugin?.guard.state===3?'就绪':plugin?.guard.state===2?'排队':plugin?.guard.state===1?'已禁用':'未知'} / {plugin?.guard.last_result??'—'}</span></div>
    {#if plugin?.guard.error}<div class="row"><span class="row-l"><b>任务错误</b></span><span class="row-v er">{plugin.guard.error}</span></div>{/if}
    {#if plugin?.guard.installed && !plugin.guard.verified}<div class="row"><span class="row-l"><b>核验</b></span><span class="row-v er">任务参数或程序版本未通过核验，管理按钮不可用。</span></div>{/if}
    <div class="row foot">
      <span class="hint">暂停不会停用补丁，也不会中止已开始的检查。启用后，后续计划检查可能在超限时重启 Central 服务。</span>
      <div class="actions">
        <button type="button" class="btn" disabled={pending||busy||!authorized||!plugin?.guard.verified||!!problem||plugin.guard.enabled==null} onclick={()=>confirmation=plugin?.guard.enabled?'disable':'enable'}>{plugin?.guard.enabled?'暂停自动检查':'启用自动检查'}</button>
        {#if !authorized}<button type="button" class="btn primary" disabled={writeBlocked('auth')} aria-busy={isWriting('auth')} onclick={async()=>{await onauthorize();await refresh();}}>授权插件管理</button>{/if}
      </div>
    </div>
  </div>
  {#if confirmation}<ConfirmDialog title="确认看门狗任务变更" oncancel={()=>confirmation=null}><p>{confirmation==='enable'?'启用后将恢复计划检查，超限时可能重启 Central 服务。':'暂停后不再触发新的定期检查，正在进行的检查不会被取消。'} 是否继续？</p><div class="action-row"><button class="btn primary" disabled={pending||busy} onclick={change}>确认{confirmation==='enable'?'启用':'暂停'}</button><button data-dialog-cancel class="btn" onclick={()=>confirmation=null}>取消</button></div></ConfirmDialog>{/if}

  <div class="sh"><span>资源用量</span><span>{plugin?'采样 '+new Date(plugin.captured_at_ms).toLocaleTimeString():'等待采样'}</span></div>
  <div class="group">
    <div class="row"><span class="row-l"><b>CPU · 整机比例</b></span><span class="row-v">{@render metric(latestCpu,2)} %</span></div>
    <div class="row"><span class="row-l"><b>工作集 / 私有提交</b></span><span class="row-v">{@render memoryMetric(metrics?.working_set_bytes)} / {@render memoryMetric(metrics?.private_commit_bytes)}</span></div>
    <div class="row"><span class="row-l"><b>句柄 / 线程</b></span><span class="row-v">{@render metric(metrics?.handles)} / {@render metric(metrics?.threads)}</span></div>
    <div class="row"><span class="row-l"><b>运行时长</b></span><span class="row-v">{@render runtime()}</span></div>
    {#if plugin?.errors.length}<div class="row"><span class="row-l"><b>指标错误</b></span><span class="row-v er">{plugin.errors.join('；')} · 无权限的指标保持未知，不按零计算。</span></div>{/if}
    <div class="disc-row"><Disclosure title="本页采样统计"><div class="pad detail">
      <p>保留样本 / 时间跨度：{@render metric(samples.length)} / {@render metric(span/60,1)} 分钟</p>
      <p>CPU 时间加权均值：{@render metric(averageCpu,2)} %</p>
      <p>工作集均值 / 峰值：{@render memoryMetric(meanMemory)} / {@render memoryMetric(peakMemory)}</p>
      <p>句柄净变化速率：{@render metric(handlesPerMinute,2)} / 分钟</p>
      <p class="hint">只统计 AASSvc 进程，最多保留最近 120 个样本。进程重启后重新计数；CPU 至少需要两个有效样本。工作集包含共享页，私有提交不是实际驻留内存。净变化不能单独证明泄漏。</p>
      <div><button type="button" class="btn" onclick={()=>{samples=[];latestCpu=null;}}>重置统计</button></div>
    </div></Disclosure></div>
  </div>

  <div class="sh"><span>检查记录与日志</span></div>
  <div class="group">
    <div class="row"><label class="row-l" for="central-log-switch"><b>查看日志</b><small>日志默认不读取。开启查看后按需加载；收起即清空页面缓存。</small></label><Switch id="central-log-switch" bind:checked={showLogs} onchange={toggleLogs}/></div>
    <Collapse open={showLogs}>
      <div class="pad">
        <div class="log-bar">
          <label class="hint" for="central-log">日志来源</label>
          <PopupSelect id="central-log" aria-label="日志类型" bind:value={logKind} onchange={readLog} options={[{value:'events',label:'重启与异常'},{value:'previous_events',label:'上一份轮转日志'},{value:'last_check',label:'最近检查结果'},{value:'activation',label:'补丁启用记录'}]}/>
          <button type="button" class="btn" disabled={logPending} onclick={readLog}>{@render busyLabel(logPending, '读取中…', '刷新日志')}</button>
          {#if logResult}<span class="hint log-size">{logResult.truncated?'只显示末尾 32 KiB 内的完整行':'文件内容'} · {@render metric(logResult.file_bytes)} 字节</span>{/if}
        </div>
        {#if logError}<p role="status" class="hint er">{logError}</p>{/if}
        {#if logResult}<textarea class="logbox mono" readonly aria-label="日志内容" rows="12" value={logResult.text||'日志为空'}></textarea>{/if}
      </div>
    </Collapse>
    <div class="row"><span class="row-l"><small>上次计划运行 {date(plugin?.guard.last_run)} · 下次计划运行 {date(plugin?.guard.next_run)}</small></span></div>
  </div>
</div>

<style>
  .plugin-page{padding-bottom:12px}
  .plugin-page > .group:first-child + .sh{margin-top:28px}
  .sq svg{width:15px;height:15px;stroke-width:2}
  p{margin:0}
  p.hint{overflow-wrap:anywhere}
  .er{color:var(--red-text, var(--danger))}
  .row > .btn{flex-shrink:0}
  .row-v{max-width:60%}
  .actions{display:flex;flex-wrap:wrap;gap:8px;flex-shrink:0}
  .disc-row{position:relative}
  .disc-row::before{content:"";position:absolute;top:0;left:18px;right:0;height:1px;background:var(--sep, var(--line));transform:scaleY(.5);transform-origin:top}
  .detail{display:grid;gap:4px;padding-top:0;font-size:12px;line-height:1.7;color:var(--label-2, var(--secondary));overflow-wrap:anywhere;word-break:break-all}
  .log-bar{display:flex;flex-wrap:wrap;gap:8px;align-items:center;margin-bottom:10px}
  .log-size{margin-left:auto}
  .logbox{display:block;width:100%;min-height:160px;max-height:320px;border:0;border-radius:10px;padding:12px 14px;background:var(--fill-2, var(--raised));color:inherit;resize:vertical;white-space:pre-wrap;overflow-wrap:anywhere;overflow:auto;line-height:1.6;user-select:text}
  @media(max-width:780px){.row{flex-wrap:wrap}.row-v{max-width:none;text-align:left}.row.foot{flex-wrap:wrap}}
</style>
