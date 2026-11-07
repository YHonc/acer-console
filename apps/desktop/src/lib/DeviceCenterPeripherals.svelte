<script lang="ts">
  import { tick, onMount } from 'svelte';
  import { dc, setOrder, acceptModule } from './dcState.svelte';
  import { dcApi } from './dcApi';
  import { beginWrite } from './writeResources.svelte';
  import DeviceCenterSettings from './DeviceCenterSettings.svelte';
  import Collapse from './ui/Collapse.svelte';
  import Switch from './ui/Switch.svelte';
  import Icon from './Icon.svelte';
  let soon=$state(false), busy=$state(''), announcement=$state(''), keepTarget=$state(false), host:HTMLDivElement;
  let drag=$state<{id:string;pid:number;row:HTMLElement;rows:HTMLElement[];rects:DOMRect[];from:number;to:number;y:number;moved:boolean}|null>(null);
  const reduced=()=>matchMedia('(prefers-reduced-motion: reduce)').matches;
  onMount(()=>{const media=matchMedia('(prefers-reduced-motion: reduce)');const cancel=()=>{if(media.matches)host?.getAnimations({subtree:true}).forEach(animation=>animation.cancel());};media.addEventListener('change',cancel);return()=>media.removeEventListener('change',cancel);});
  const ordered=$derived(dc.order.map(id=>dc.devices.find(d=>d.id===id)).filter(d=>d!==undefined));
  const moduleState=$derived(dc.module?.state??'stopped');
  const pid=$derived(dc.module?.pid?` · PID ${dc.module.pid}`:'');
  const protocol=$derived(dc.module?.protocol?`${dc.module.protocol.major}.${dc.module.protocol.minor}`:'—');
  const view=$derived.by(()=>{
    if(!dc.module)return {tag:'—',kind:'',proc:'—',proto:'—',tone:'',title:'',note:'',action:''};
    const exit=dc.module?.exitCode;
    return ({
      absent:{tag:'未发现',kind:'',proc:'未运行',proto:'—',tone:'info',title:'模块未运行',note:'外设控件在模块启动后可用。',action:''},
      untrusted:{tag:'待确认',kind:'draft',proc:'未运行',proto:'—',tone:'',title:'模块文件已变更，确认后启动',note:'文件哈希与上次信任的快照不一致。确认前不运行模块。',action:'确认并启动'},
      running:{tag:'运行中',kind:'ok',proc:'运行中'+pid,proto:protocol+` · 一期能力 ${dc.module?.capabilities.length??0} 项`,tone:'',title:'',note:'',action:''},
      starting:{tag:'启动中',kind:'',proc:'启动中'+pid,proto:'等待握手',tone:'info',title:'正在启动模块',note:'等待模块完成握手。启动前不显示任何外设读数。',action:''},
      reconnecting:{tag:'重连中',kind:'draft',proc:'运行中'+pid,proto:protocol+' · 连接中断',tone:'info',title:`连接中断，正在重连（第 ${dc.module?.reconnectAttempt??'—'}/3 次）`,note:'后台进程仍在运行。重连前不更新读数，也不发送写入。',action:''},
      stopped:{tag:'已停止',kind:'',proc:'未运行',proto:'—',tone:'info',title:'模块未运行',note:'外设控件在模块启动后可用。',action:'启动模块'},
      elsewhere:{tag:'被占用',kind:'draft',proc:'其他实例运行中',proto:'—',tone:'',title:'Device Center 已在其他地方运行',note:'可能是独立窗口版，或上次保持在后台的模块。关闭它后再重新检测，控制台不会强行接管。',action:'重新检测'},
      incompatible:{tag:'不兼容',kind:'er',proc:'已退出',proto:`模块 ${protocol} · 控制台支持 1.x`,tone:'er',title:'协议版本不兼容',note:`模块协议主版本为 ${dc.module?.protocol?.major??'—'}，本控制台支持 1.x。请更新控制台或换回匹配的模块。`,action:'重新检测'},
      error:{tag:'异常',kind:'er',proc:'已退出'+(exit!=null?` · 退出码 ${exit}`:''),proto:'—',tone:'er',title:'模块异常退出',note:`退出码 ${exit??'—'}${exit===10?'（内部错误）':''}。外设读数已清空，不显示旧值；写入结果以重启后的读回为准。`,action:'重新启动'}
    })[moduleState];
  });
  async function moduleAction(kind:string,keep?:boolean){if(busy)return;const release=beginWrite('deviceCenter');if(!release)return;keepTarget=!!keep;busy=kind;
    try{acceptModule(await (kind==='keep'?dcApi.keep(!!keep):kind==='重新检测'?dcApi.status():dcApi.start()));}catch(e){dc.error=e instanceof Error?e.message:String(e);}finally{busy='';release();}}
  const spring=()=>getComputedStyle(document.documentElement).getPropertyValue('--spring-snappy').trim();
  async function move(id:string,index:number,animate=true){const ids=ordered.map(d=>d.id);const from=ids.indexOf(id);if(from<0)return;index=Math.max(0,Math.min(ids.length-1,index));if(index===from)return;
    const rows=Array.from(host.querySelectorAll<HTMLElement>('.dev-row'));const before=new Map(rows.map(r=>[r,r.getBoundingClientRect().top]));
    ids.splice(from,1);ids.splice(index,0,id);setOrder([...ids,...dc.order.filter(id=>!ids.includes(id))]);announcement=`${dc.devices.find(d=>d.id===id)?.name}移到第 ${index+1} 位`;await tick();
    host.querySelector<HTMLButtonElement>(`[data-id="${CSS.escape(id)}"] .grip`)?.focus({preventScroll:true});
    if(animate&&!reduced())for(const row of rows){const dy=before.get(row)!-row.getBoundingClientRect().top;if(dy)row.animate([{transform:`translateY(${dy}px)`},{transform:'none'}],{duration:370,easing:spring()});}
  }
  function key(e:KeyboardEvent,id:string,index:number){const to=e.key==='ArrowDown'?index+1:e.key==='ArrowUp'?index-1:e.key==='Home'?0:e.key==='End'?ordered.length-1:null;if(to!=null){e.preventDefault();void move(id,to);}}
  function start(e:PointerEvent,id:string){if(e.button!==0)return;e.preventDefault();const button=e.currentTarget as HTMLElement;button.setPointerCapture(e.pointerId);button.focus();const rows=Array.from(host.querySelectorAll<HTMLElement>('.dev-row'));const row=button.closest<HTMLElement>('.dev-row')!;const from=rows.indexOf(row);drag={id,pid:e.pointerId,row,rows,rects:rows.map(r=>r.getBoundingClientRect()),from,to:from,y:e.clientY,moved:false};}
  function pointer(e:PointerEvent){const d=drag;if(!d||d.pid!==e.pointerId)return;let dy=e.clientY-d.y;if(!d.moved){if(Math.abs(dy)<3)return;d.moved=true;}
    const R=d.rects,f=d.from;dy=Math.max(R[0].top-R[f].top,Math.min(R.at(-1)!.bottom-R[f].bottom,dy));d.row.style.transform=`translateY(${dy}px)`;
    const mid=(r:DOMRect)=>r.top+r.height/2,c=mid(R[f])+dy;let to=f;R.forEach((r,i)=>{if(i<f&&c<=mid(r)+.5)to=Math.min(to,i);if(i>f&&c>=mid(r)-.5)to=Math.max(to,i);});d.to=to;
    d.rows.forEach((r,i)=>{if(i===f)return;const offset=f<to&&i>f&&i<=to?-R[f].height:f>to&&i>=to&&i<f?R[f].height:0;r.style.transform=offset?`translateY(${offset}px)`:'';});
  }
  async function end(e:PointerEvent){const d=drag;if(!d||d.pid!==e.pointerId)return;drag=null;if(!d.moved)return;const visible=d.row.getBoundingClientRect().top;d.rows.forEach(r=>r.style.transform='');if(d.to!==d.from)await move(d.id,d.to,false);const dy=visible-d.row.getBoundingClientRect().top;if(dy&&!reduced())d.row.animate([{transform:`translateY(${dy}px)`},{transform:'none'}],{duration:370,easing:spring()});}
</script>

<div class="sh"><span>模块</span><span class="tag {view.kind}" id="dc-mod-tag">{view.tag}</span></div>
<div class="group">
  <div class="row"><span class="sq indigo"><Icon name="cube"/></span><span class="row-l"><b>Device Center 模块</b><small class="mono">{!dc.module?'—':moduleState==='absent'?'未发现 modules/device-center':'已发现 modules/device-center'}{dc.module?.moduleVersion?' · v'+dc.module.moduleVersion:''}</small></span></div>
  <div class="row"><span class="row-l"><b>后台进程</b></span><span class="row-v" id="dc-proc">{view.proc}</span></div>
  <div class="row"><span class="row-l"><b>协议版本</b><small>主版本不一致时拒绝连接</small></span><span class="row-v" id="dc-proto">{view.proto}</span></div>
  <Collapse open={!!view.title}><div class="conflict {view.tone}" id={moduleState==='untrusted'?'dc-hash':'dc-note'} role="status">
    {#if moduleState==='starting'||moduleState==='reconnecting'}<span class="spinner" aria-hidden="true"></span>{:else}<Icon name="warn"/>{/if}
    <span class="row-l"><b>{view.title}</b><small>{view.note}</small></span>{#if view.action}<span class="cf-act"><button class="btn" class:primary={view.action!=='重新检测'} disabled={!!busy} onclick={()=>moduleAction(view.action)}>{#if busy===view.action}<span class="spinner"></span>{/if}{view.action}</button></span>{/if}
  </div></Collapse>
  <div class="row"><span class="row-l"><b>控制台退出时保持后台运行</b><small>关闭后，控制台退出时一并停止模块进程。</small></span><Switch aria-label="控制台退出时保持后台运行" checked={busy==='keep'?keepTarget:dc.module?.keep??false} pending={busy==='keep'} disabled={!dc.module} onchange={e=>moduleAction('keep',e.currentTarget.checked)}/></div>
</div>
{#if dc.error}<p class="hint err-text" role="status">{dc.error}</p>{/if}
<div class="sh"><span>设备</span><span id="dc-sync-note">{#if dc.reading}<span class="sync-ind"><span class="spinner"></span>同步中 · 控件仍可编辑</span>{:else}{moduleState==='running'?'设备读回':'模块未运行'}{/if}</span></div>
<div class="group" id="dc-devs" bind:this={host} class:sorting={drag?.moved} role="list" aria-label="设备，可拖动把手或用上下方向键调整顺序">
  {#each ordered as device,index (device.id)}
    <div class="row dev-row" class:off={device.connected===false} class:dragging={drag?.id===device.id&&drag.moved} data-id={device.id} role="listitem">
      <button class="grip" type="button" aria-label={'调整顺序：'+device.name} aria-describedby="dc-order-help" onkeydown={e=>key(e,device.id,index)} onpointerdown={e=>start(e,device.id)} onpointermove={pointer} onpointerup={end} onpointercancel={end}>
        <svg viewBox="0 0 8 14" aria-hidden="true">{#each [2,7,12] as y}<circle cx="2" cy={y} r="1"/><circle cx="6" cy={y} r="1"/>{/each}</svg>
      </button>
      <span class="sq {device.kind==='mouse'?'blue':device.kind==='kb'?'gray':'orange'}"><Icon name={device.kind==='kb'?'key':device.kind==='mouse'?'mouse':'lamp'} size={15}/></span>
      <span class="row-l"><b>{device.name}</b><small><i class="dot" class:ok={moduleState==='running'&&device.connected===true}></i> {moduleState!=='running'||device.connected==null?'状态未知':device.stale?'读数过期':device.connected?'已连接':'已断开'}{device.connected===true&&moduleState==='running'?' · '+(device.kind==='mouse'?'2.4G 接收器':device.kind==='kb'?'有线 USB':'局域网 Wi-Fi'):''}</small></span><span class="row-v">电量 {moduleState==='running'&&device.connected&&device.battery!=null?device.battery+'%':'—'}</span>
    </div>
  {/each}
</div>
<p class="hint" style="margin:8px 6px 0" id="dc-order-help">拖动行首把手，或选中把手后按 ↑ ↓ 调整顺序；下方设置分组按同一顺序排列。</p>
<div class="vh" aria-live="polite" id="dc-live">{announcement}</div>
<div id="dc-secs">{#each ordered as device (device.id)}<DeviceCenterSettings {device}/>{/each}</div>
<div class="group" style="margin-top:20px"><button class="disc" aria-expanded={soon} aria-controls="dc-soon" onclick={()=>soon=!soon}>即将支持（二期）<Icon name="chev"/></button>
  <Collapse open={soon}><div id="dc-soon">{#each [{label:'鼠标宏',note:'录制、编辑并绑定到按键'},{label:'键盘按键层',note:'多层按键映射'},{label:'小米云登录',note:'从小米账号导入挂灯，免手填 Token'},{label:'鼠标性能项',note:'抬升高度、静默高度等'}] as item}<div class="row soon"><span class="row-l"><b>{item.label}</b><small>{item.note}</small></span><span class="tag">即将支持</span></div>{/each}</div></Collapse>
</div>
