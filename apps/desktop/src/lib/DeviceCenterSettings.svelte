<script lang="ts">
  import { untrack } from 'svelte';
  import { dc, capable, invalidateDcRead, type DcDevice } from './dcState.svelte';
  import { dcApi } from './dcApi';
  import type { DcSettings, DcDpi, DcLighting, DcLampState, DcMethod } from './dcTypes';
  import { beginWrite } from './writeResources.svelte';
  import SegmentedControl from './ui/SegmentedControl.svelte';
  import PopupSelect from './ui/PopupSelect.svelte';
  import Switch from './ui/Switch.svelte';
  import Collapse from './ui/Collapse.svelte';
  import ReadConflict from './ui/ReadConflict.svelte';
  import Icon from './Icon.svelte';
  let { device }: {device:DcDevice}=$props();
  type Values=Record<string,number|string|boolean|null>;
  let draft=$state<Values>({}), edited=$state<Record<string,boolean>>({}), pending=$state<string[]>([]), errors=$state<Record<string,string>>({}), requested=$state<Record<string,boolean>>({});
  let ip=$state(''),token=$state(''),tokenEdit=$state(false),cfgEdited=$state(false);
  const live=$derived(dc.module?.state==='running'&&device.connected===true&&!device.stale);
  const settings=$derived(live?dc.settings[device.id]:undefined);
  const dpi=$derived(live?dc.dpi[device.id]:undefined);
  const lighting=$derived(live?dc.lighting[device.id]:undefined);
  const lamp=$derived(live&&device.kind==='lamp'?dc.lamp:null);
  const read=$derived.by(()=>{
    const v:Values={};for(const f of settings?.fields??[])if(f.available)v[f.key]=f.value;
    if(dpi){const level=typeof v.dpiLevel==='number'?v.dpiLevel:dpi.currentLevel;v.dpi=dpi.levels.find(l=>l.level===level)?.xDpi??null;}
    if(lighting){v.mode=lighting.mode;v.brightness=lighting.brightness;v.speed=lighting.speed;v.multiColor=lighting.multiColor;v.rgb=lighting.rgb?'#'+lighting.rgb.map(v=>v.toString(16).padStart(2,'0')).join(''):null;}
    if(lamp){v.brightness=lamp.brightness;v.temperature=lamp.temperature;v.power=lamp.power;}
    return v;
  });
  $effect(()=>{const values=read;untrack(()=>{for(const key of new Set([...Object.keys(draft),...Object.keys(values)])){const value=values[key]??null;if(!edited[key]||draft[key]===value){draft[key]=value;edited[key]=false;}}});});
  $effect(()=>{const cfg=dc.config;if(!untrack(()=>cfgEdited))ip=cfg?.ip??'';});
  $effect(()=>{if(dc.module?.state!=='running'){token='';tokenEdit=false;}});
  const seenReadbacks:Record<string,number>={};
  function recoveryCount(method:string){return (dc.readbacks[`${device.id}:${method}`]??0)+(dc.readbacks[`${device.id}:device.snapshots`]??0)+(dc.readbacks['all:device.snapshots']??0);}
  $effect(()=>{
    const groups:Record<string,string[]>={'settings.read':[device.kind==='mouse'?'mouse':'kb','lowLatency','macMode','winLock'],'mouse.dpi.read':['mouse'],'keyboard.lighting.read':['kbl'],'lamp.read':['lamp','power'],'lamp.config.read':['config']};
    for(const [method,keys] of Object.entries(groups)){
      const count=recoveryCount(method);
      if(count!==(seenReadbacks[method]??0)){seenReadbacks[method]=count;untrack(()=>{for(const key of keys)if(errors[key]==='结果未知，请刷新读回')errors[key]='';});}
    }
  });
  /* 挂灯连接：未配置时直接要求输入 Token（不必先点“更换”）；已配置时 Token 省略表示保留原值。 */
  const unconfigured=$derived(dc.config?.configured===false);
  const tokenInput=$derived(tokenEdit||unconfigured);
  const configDirty=$derived(cfgEdited&&(ip!==(dc.config?.ip??'')||tokenInput&&!!token));
  const ipBad=$derived(!/^(\d{1,3}\.){3}\d{1,3}$/.test(ip)||ip.split('.').some(v=>+v>255));
  const tokenBad=$derived(tokenInput&&!/^[0-9a-fA-F]{32}$/.test(token));
  let connOpen=$state<boolean|null>(null);
  const connShown=$derived(connOpen??(unconfigured||configDirty||!!errors.config));
  function edit(key:string,value:number|string|boolean){draft[key]=value;edited[key]=true;}
  function fields(group:string){return group==='mouse'?['dpi','reportRateHz']:group==='kb'?['reportRateHz','sleepSeconds']:group==='kbl'?['mode','brightness','speed','multiColor','rgb']:['brightness','temperature'];}
  const labels:Record<string,string>={dpi:'DPI',reportRateHz:'回报率',sleepSeconds:'休眠',mode:'模式',brightness:'亮度',speed:'速度',multiColor:'颜色',rgb:'RGB',temperature:'色温'};
  function format(key:string,value:Values[string]){if(value==null)return '—';if(key==='sleepSeconds')return Number(value)===0?'从不':`${Number(value)/60} 分钟`;if(key==='reportRateHz')return value+' Hz';if(key==='temperature')return value+'K';if(key==='brightness')return value+(device.kind==='lamp'?'%':' 档');if(key==='speed')return value+' 档';if(key==='multiColor')return value?'多彩':'单色';if(key==='mode')return lighting?.modes.find(m=>m.id===value)?.label??'—';return String(value).toUpperCase();}
  function differences(group:string,values:Values){return fields(group).filter(k=>draft[k]!==read[k]).map(k=>`${labels[k]} ${format(k,values[k])}`).join(' · ');}
  const unknownHint=$derived(dc.module?.state==='starting'?'模块正在启动，暂无设备读数。':dc.module?.state==='reconnecting'?'与模块的连接已中断，读数暂停更新。':dc.module?.state==='elsewhere'?'Device Center 已在其他地方运行，暂无设备读数。':dc.module?.state==='incompatible'?'协议版本不兼容，暂无设备读数。':dc.module?.state==='error'?'模块异常退出，旧读数已清空。':'模块未运行，暂无设备读数。');
  function dirty(group:string){return fields(group).some(k=>edited[k]&&draft[k]!==read[k]);}
  function groupReady(group:string){return live&&(group==='kbl'?!!lighting:group==='lamp'?!!lamp:!!settings);}
  function summary(group:string){const sleep=Number(read.sleepSeconds)===0?'从不':`${Number(read.sleepSeconds)/60} 分钟`;return group==='mouse'?`${read.dpi??'—'} DPI · ${read.reportRateHz??'—'} Hz`:group==='kb'?`${read.reportRateHz??'—'} Hz · 休眠 ${read.sleepSeconds==null?'—':sleep}`:group==='kbl'?`${lighting?.modes.find(m=>m.id===read.mode)?.label??'—'} · 亮度 ${read.brightness??'—'} 档`:`亮度 ${read.brightness??'—'}% · ${read.temperature??'—'}K`;}
  function adopt(group:string){for(const k of fields(group)){draft[k]=read[k]??null;edited[k]=false;}}
  let queue=Promise.resolve(),writeEpoch=0;
  const validWrite=()=>live&&writeEpoch===dc.epoch;
  function write(key:string,task:()=>Promise<void>){
    if((key==='config'?dc.module?.state!=='running':!live)||pending.includes(key))return;pending=[...pending,key];errors[key]='';
    queue=queue.then(async()=>{if(key==='config'?dc.module?.state!=='running':!live)return;const release=beginWrite(`dc:${device.id}`);if(!release){errors[key]='设备正在写入';return;}
      const method=key==='config'?'lamp.config.read':device.kind==='lamp'?'lamp.read':key==='kbl'?'keyboard.lighting.read':'settings.read';
      const recovery=recoveryCount(method);
      writeEpoch=dc.epoch;invalidateDcRead(device.id);try{await task();}catch(e){const message=e instanceof Error?e.message:String(e);errors[key]=message==='结果未知，请刷新读回'&&recoveryCount(method)!==recovery?'':message;}finally{invalidateDcRead(device.id);release();if(dc.epoch!==writeEpoch)errors[key]='结果未知，请刷新读回';}
    }).finally(()=>pending=pending.filter(k=>k!==key));
  }
  function apply(group:string){const change=Object.fromEntries(fields(group).filter(k=>edited[k]&&draft[k]!==read[k]).map(k=>[k,draft[k]]));
    write(group,async()=>{
      if(group==='mouse'||group==='kb'){
        if(change.dpi!=null){const current=dc.dpi[device.id];if(!current)throw Error('结果未知，请刷新读回');const level=current.levels.find(l=>l.xDpi===change.dpi);if(!level)throw Error('DPI 档位不可用');
          change.dpiLevel=level.level;delete change.dpi;}
        if(Object.keys(change).length){const s=dc.settings[device.id];const next=await dcApi.call<DcSettings>('settings.apply',{deviceId:device.id,change:{...change,expectedState:s?.stateToken??undefined,expectedReportRateHz:s?.fields.find(f=>f.key==='reportRateHz')?.value??undefined,expectedDpiLevel:s?.fields.find(f=>f.key==='dpiLevel')?.value??undefined}});if(validWrite())dc.settings[device.id]=next;}
      }else if(group==='kbl'){
        if(typeof change.rgb==='string')change.rgb=change.rgb;
        const payload:Record<string,unknown>={...change,expectedState:dc.lighting[device.id]?.stateToken};
        if(typeof change.rgb==='string')payload.rgb=[1,3,5].map(i=>parseInt(String(change.rgb).slice(i,i+2),16));
        const next=await dcApi.call<DcLighting>('keyboard.lighting.apply',{deviceId:device.id,change:payload});if(validWrite())dc.lighting[device.id]=next;
      }else {const next=await dcApi.call<DcLampState>('lamp.apply',{change});if(validWrite())dc.lamp=next;}
    });
  }
  function instant(key:string,value:boolean){requested[key]=value;write(key,async()=>{if(device.kind==='lamp'){const next=await dcApi.call<DcLampState>('lamp.apply',{change:{power:value}});if(validWrite())dc.lamp=next;}
    else{const next=await dcApi.call<DcSettings>('settings.apply',{deviceId:device.id,change:{[key]:value,expectedState:dc.settings[device.id]?.stateToken??undefined}});if(validWrite())dc.settings[device.id]=next;}});}
  function options(key:string,fallback:number[]){const values=settings?.fields.find(f=>f.key===key)?.options?.map(o=>o.value)??fallback;return values.map((value,i)=>({value:String(value),label:String(value)+(i===values.length-1?' Hz':'')}));}
  function visible(key:string,method:DcMethod){return capable(method)&&(!settings||settings.fields.some(f=>f.key===key&&f.available));}
  const effectMode=$derived(lighting?.modes.find(m=>m.id===Number(draft.mode)));
  /** 滑块填充：按草稿值在 [min,max] 中的位置；无读数时不填充。 */
  function pct(value:Values[string],min:number,max:number){const n=Number(value);return value==null||!Number.isFinite(n)||max<=min?'0%':Math.min(100,Math.max(0,(n-min)/(max-min)*100))+'%';}
  function canApply(group:string){return capable(group==='kbl'?'keyboard.lighting.apply':group==='lamp'?'lamp.apply':'settings.apply');}
</script>

{#snippet title(label:string,group:string)}
  <div class="sh"><span>{label}</span><span class="tag" class:ok={groupReady(group)&&!dirty(group)} class:draft={groupReady(group)&&dirty(group)}>{!groupReady(group)?(device.stale?'读数过期':dc.module?.state==='running'&&device.connected===false?'设备已断开':'状态未知'):dirty(group)?'待应用 · 设备 '+summary(group):'当前 · '+summary(group)}</span></div>
{/snippet}
{#snippet segment(label:string,key:string,opts:{value:string;label:string}[],ariaLabel=label)}
  <div class="row"><span class="row-l"><b>{label}</b></span><SegmentedControl size="small" label={ariaLabel} value={live&&draft[key]!=null?String(draft[key]):''} current={read[key]!=null?String(read[key]):''} options={opts} disabled={!live} onchange={v=>edit(key,Number(v))}/></div>
{/snippet}
{#snippet footer(group:string,label='应用')}
  <ReadConflict readKey={live?JSON.stringify(fields(group).map(k=>read[k])):null} edited={groupReady(group)&&fields(group).some(k=>edited[k])} matches={!dirty(group)} label={differences(group,read)} note="同步时读回发生变化，草稿没有被覆盖。" icon onadopt={()=>adopt(group)}/>
  <div class="row foot"><span class="hint" role="status">{errors[group]||(!live?(dc.module?.state==='running'?'设备已断开，暂无读数；重新连接后恢复。':unknownHint):dirty(group)?'待应用：'+differences(group,draft):'与设备读回一致。应用前不会改动设备。')}</span>{#if canApply(group)}<button class="btn primary" disabled={!groupReady(group)||!dirty(group)||pending.includes(group)} onclick={()=>apply(group)}>{#if pending.includes(group)}<span class="spinner"></span>{/if}{label}</button>{/if}</div>
{/snippet}
{#snippet toggle(label:string,key:string,note='')}
  <div class="row"><span class="row-l"><b>{label}</b>{#if note}<small>{note}</small>{/if}{#if errors[key]}<small role="status">{errors[key]}</small>{/if}</span>
    {#if live&&read[key]!=null}<Switch aria-label={label} checked={pending.includes(key)?requested[key]:!!read[key]} pending={pending.includes(key)} onchange={e=>instant(key,e.currentTarget.checked)}/>{:else}<span class="row-v">—</span>{/if}
  </div>
{/snippet}
{#snippet slider(label:string,key:string,min:number,max:number,step=1,enabled=true,suffix='')}
  <div class="slider-row"><label for={device.id+'-'+key}>{label}</label><input id={device.id+'-'+key} class="slider" type="range" {min} {max} {step} style:--p={pct(draft[key],min,max)} value={Number(draft[key]??min)} disabled={!live||!enabled||read[key]==null} oninput={e=>edit(key,+e.currentTarget.value)}/><output>{live&&draft[key]!=null?String(draft[key])+suffix:'—'}</output></div>
{/snippet}

<div class="dc-sec" data-sec={device.id}>
{#if device.kind==='mouse'}
  {@render title(device.name,'mouse')}
  <div class="group" data-g="mouse">
    {#if capable('mouse.dpi.read')}<div class="row"><span class="row-l"><b>当前 DPI</b><small>设备读回</small></span><span class="row-v strong">{read.dpi??'—'}</span></div>{/if}
    {#if capable('mouse.dpi.read')&&visible('dpiLevel','settings.apply')}{@render segment('DPI 档位','dpi',dpi?.levels.map(l=>({value:String(l.xDpi),label:String(l.xDpi)}))??[])}{/if}
    {#if visible('reportRateHz','settings.apply')}{@render segment('回报率','reportRateHz',options('reportRateHz',[125,500,1000,2000]),'鼠标回报率')}{/if}
    {#if capable('mouse.mapping.read')}<button class="row" disabled={!live} onclick={async()=>{try{await dcApi.call('mouse.mapping.read',{deviceId:device.id});}catch(e){errors.mouse=e instanceof Error?e.message:String(e);}}}><span class="row-l"><b>按键映射</b><small>读取自设备</small></span><span class="chev">›</span></button>{/if}
    <div class="row"><span class="row-l"><small style="margin:0">鼠标灯效：暂不支持</small></span></div>
    {@render footer('mouse')}
  </div>
{:else if device.kind==='kb'}
  {@render title(device.name,'kb')}
  {#if capable('settings.read')}<div class="group" data-g="kb">
    {#if visible('reportRateHz','settings.apply')}{@render segment('回报率','reportRateHz',options('reportRateHz',[125,250,500,1000]),'键盘回报率')}{/if}
    {#if visible('sleepSeconds','settings.apply')}<div class="row"><span class="row-l"><b>休眠时间</b><small>无操作后进入休眠</small></span><PopupSelect aria-label="休眠时间" value={Number(draft.sleepSeconds??0)} current={read.sleepSeconds==null?null:Number(read.sleepSeconds)} unknown={!live||read.sleepSeconds==null} disabled={!live} options={[{value:60,label:'1 分钟'},{value:300,label:'5 分钟'},{value:600,label:'10 分钟'},{value:1800,label:'30 分钟'},{value:0,label:'从不'}]} onchange={e=>edit('sleepSeconds',+e.currentTarget.value)}/></div>{/if}
    {@render footer('kb')}
  </div>{/if}
  {#if capable('settings.apply')}<div class="group" style="margin-top:12px">
    {#if visible('lowLatency','settings.apply')}{@render toggle('低延迟模式','lowLatency','点击后立即写入并读回')}{/if}
    {#if visible('macMode','settings.apply')}{@render toggle('Mac 模式','macMode')}{/if}
    {#if visible('winLock','settings.apply')}{@render toggle('Win 锁','winLock','锁定 Windows 键')}{/if}
  </div>{/if}
  {#if capable('keyboard.lighting.read')}
    {@render title(device.name+'灯效','kbl')}
    <div class="group" data-g="kbl">
      {#if capable('keyboard.lighting.apply')}
      <div class="row"><span class="row-l"><b>模式</b></span><PopupSelect aria-label="键盘灯效模式" value={Number(draft.mode??0)} current={read.mode==null?null:Number(read.mode)} unknown={!live||read.mode==null} disabled={!live} options={lighting?.modes.map(m=>({value:m.id,label:m.label}))??[]} onchange={e=>edit('mode',+e.currentTarget.value)}/></div>
      {@render slider('亮度','brightness',1,4,1,capable('keyboard.lighting.apply')&&!!effectMode?.brightnessEditable)}
      {@render slider('速度','speed',1,4,1,capable('keyboard.lighting.apply')&&!!effectMode?.speedEditable)}
      <div class="row"><span class="row-l"><b>颜色</b></span><SegmentedControl size="small" label="单色或多彩" value={live&&draft.multiColor!=null?(draft.multiColor?'multi':'mono'):''} current={read.multiColor==null?'':read.multiColor?'multi':'mono'} options={[{value:'mono',label:'单色'},{value:'multi',label:'多彩'}]} disabled={!live||!effectMode?.colorEditable} onchange={v=>edit('multiColor',v==='multi')}/></div>
      <Collapse open={live&&draft.multiColor===false}><div class="row"><span class="row-l"><b>RGB</b><small class="mono">{draft.rgb??'—'}</small></span><input class="cpick" type="color" aria-label="单色 RGB" value={String(draft.rgb??'#000000')} disabled={!live||!effectMode?.colorEditable} oninput={e=>edit('rgb',e.currentTarget.value)}/></div></Collapse>
      {/if}
      {@render footer('kbl','应用灯效')}
    </div>
  {/if}
{:else if device.kind==='lamp'}
  {#if capable('lamp.read')||capable('lamp.config.read')}
    <div class="sh"><span>{device.name}</span><span class="tag" class:ok={groupReady('lamp')&&!dirty('lamp')&&!configDirty} class:draft={configDirty||groupReady('lamp')&&dirty('lamp')}>{unconfigured?'未配置连接':configDirty?'连接待保存':!groupReady('lamp')?(device.stale?'读数过期':dc.module?.state==='running'&&device.connected===false?'设备已断开':'状态未知'):dirty('lamp')?'待应用 · 设备 '+summary('lamp'):'当前 · '+summary('lamp')}</span></div>
    <div class="group" data-g="lamp">
      {#if capable('lamp.read')}
        {#if capable('lamp.apply')}{@render toggle('挂灯开关','power','点击后立即写入并读回')}
          {@render slider('亮度','brightness',1,100,1,true,'%')}
          {@render slider('色温','temperature',2700,6500,100,true,' K')}{/if}
        {#if unconfigured}
          <div class="row foot"><span class="hint" role="status">尚未配置连接，填写下方 IP 地址和 Token 并保存后才能读取挂灯。</span></div>
        {:else}{@render footer('lamp')}{/if}
      {/if}
      {#if capable('lamp.config.read')}
        <button class="row lamp-conn" type="button" aria-expanded={connShown} aria-controls={device.id+'-conn'} onclick={()=>connOpen=!connShown}>
          <span class="sq gray"><Icon name="settings" size={15}/></span>
          <span class="row-l"><b>连接</b><small>{dc.module?.state!=='running'||!dc.config?'—':dc.config.configured?`${dc.config.ip??'—'} · Token 末 4 位 ${dc.config.tokenTail??'—'}`:'未配置'}</small></span>
          {#if configDirty}<span class="tag draft">待保存</span>{/if}
          <span class="chev"><Icon name="chev" size={14}/></span>
        </button>
        <Collapse open={connShown} id={device.id+'-conn'}>
          <div class="row"><span class="row-l"><b>IP 地址</b><small>局域网地址，路由器重新分配后需更新</small></span>{#if capable('lamp.config.save')}<input class="field mono" aria-label="挂灯 IP 地址" spellcheck="false" value={dc.module?.state==='running'?ip:''} placeholder="—" disabled={dc.module?.state!=='running'} oninput={e=>{ip=e.currentTarget.value;cfgEdited=true;}}/>{:else}<span class="row-v">{dc.config?.ip??'—'}</span>{/if}</div>
          <div class="row"><span class="row-l"><b>Token</b><small>使用 Windows DPAPI 加密保存，界面只显示末 4 位</small></span>{#if tokenInput&&capable('lamp.config.save')}<input class="field mono" type="password" aria-label="新的挂灯 Token" placeholder="32 位十六进制" disabled={dc.module?.state!=='running'} bind:value={token} oninput={()=>cfgEdited=true}/>{:else}<span class="row-v mono">{dc.config?.tokenTail?'•••• •••• •••• '+dc.config.tokenTail:'—'}</span>{/if}{#if capable('lamp.config.save')&&!unconfigured}<button class="btn" disabled={dc.module?.state!=='running'} onclick={()=>{tokenEdit=!tokenEdit;token='';}}>{tokenEdit?'取消':'更换'}</button>{/if}</div>
          <ReadConflict readKey={dc.config?JSON.stringify(dc.config):null} edited={cfgEdited} matches={!configDirty} label={dc.config?.ip??'—'} onadopt={()=>{ip=dc.config?.ip??'';token='';tokenEdit=false;cfgEdited=false;}}/>
          <div class="row foot"><span class="hint" role="status">{errors.config||(dc.module?.state!=='running'?unknownHint:ipBad?'IP 地址格式不正确。':tokenBad?'Token 应为 32 位十六进制。':configDirty?'有未保存的连接设置；保存后重新连接挂灯。':'与已保存配置一致。保存后重新连接挂灯。')}</span>{#if capable('lamp.config.save')}<button class="btn primary" disabled={dc.module?.state!=='running'||!configDirty||ipBad||tokenBad||pending.includes('config')} onclick={()=>{const value={ip:ip.trim(),...(tokenInput?{token}:{})};write('config',async()=>{const result=await dcApi.call<import('./dcTypes').DcLampConfig>('lamp.config.save',value);if(dc.module?.state==='running'&&writeEpoch===dc.epoch){dc.config=result;token='';tokenEdit=false;cfgEdited=false;connOpen=null;}});}}>{#if pending.includes('config')}<span class="spinner"></span>{/if}保存连接</button>{/if}</div>
        </Collapse>
      {/if}
    </div>
  {/if}
{/if}
</div>
