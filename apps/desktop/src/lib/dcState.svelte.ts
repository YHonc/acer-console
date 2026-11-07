import { listen } from '@tauri-apps/api/event';
import { dcApi } from './dcApi';
import type { DcModuleStatus, DcMethod, DcHidDevice, DcBattery, DcSettings, DcDpi, DcLighting, DcLampConfig, DcLampState, DcEvent, DcSnapshots, DcReadback } from './dcTypes';
export type DeviceKind = 'mouse' | 'kb' | 'lamp' | 'other';
export interface DcDevice {id:string;name:string;kind:DeviceKind;connected:boolean|null;battery:number|null;stale:boolean}
export const dc = $state({
  module: null as DcModuleStatus|null, devices: [] as DcDevice[], settings: {} as Record<string,DcSettings>,
  dpi: {} as Record<string,DcDpi>, lighting: {} as Record<string,DcLighting>, lamp:null as DcLampState|null,
  config:null as DcLampConfig|null, reading:false, error:'', order:[] as string[], epoch:0,
  readbacks:{} as Record<string,number>
});
const KEY = 'acer-console-dev-order';
let generation=0, seq=0, pending:Promise<void>|null=null;
const revisions:Record<string,number>={};
export function invalidateDcRead(id:string) { revisions[id]=(revisions[id]??0)+1; }
export const capable = (method:DcMethod) => !!dc.module?.capabilities.includes(method);
export function setOrder(ids:string[]) { dc.order=ids; try {localStorage.setItem(KEY,JSON.stringify(ids));} catch { /* session order */ } }
function clearReadings() { dc.settings={};dc.dpi={};dc.lighting={};dc.lamp=null;dc.config=null;dc.devices=dc.devices.map(d=>({...d,connected:null,battery:null,stale:false})); }
export function acceptModule(status:DcModuleStatus) {
  const previous=dc.module?.state; dc.module=status;
  if(status.state!=='running') {generation++;dc.epoch++;clearReadings();}
  else if(previous!=='running') {generation++;dc.epoch++;seq=0;void refreshDc();}
}
function battery(row:DcBattery) { const d=dc.devices.find(d=>d.id===row.deviceId); if(!d)return;
  if(row.connection) d.connected=row.connection==='connected'?true:row.connection==='disconnected'?false:null; d.stale=!!row.stale;
  d.battery=d.connected && !d.stale ? row.batteryPercent??row.percentage??row.battery??null : null;
  if(!d.connected || d.stale) {delete dc.settings[d.id];delete dc.dpi[d.id];delete dc.lighting[d.id];}
}
function acceptReadback(event:DcReadback) {
  if(dc.module?.state!=='running')return;
  const {method}=event, id=event.deviceId??(method.startsWith('lamp.')?'lamp':'all');
  // A recovery result supersedes any older refresh still in flight.
  invalidateDcRead(id);
  const value=event.ok?event.result:null;
  if(method==='settings.read'){if(value)dc.settings[id]=value as DcSettings;else delete dc.settings[id];}
  else if(method==='mouse.dpi.read'){if(value)dc.dpi[id]=value as DcDpi;else delete dc.dpi[id];}
  else if(method==='keyboard.lighting.read'){if(value)dc.lighting[id]=value as DcLighting;else delete dc.lighting[id];}
  else if(method==='lamp.config.read')dc.config=value as DcLampConfig|null;
  else if(method==='lamp.read'){
    dc.lamp=value as DcLampState|null;
    const d=dc.devices.find(d=>d.id==='lamp');if(d&&value){d.connected=true;d.stale=false;}
  }else if(method==='battery.read'){
    if(value)battery({...value as DcBattery,deviceId:id});else {const d=dc.devices.find(d=>d.id===id);if(d)d.battery=null;}
  }else if(method==='device.snapshots'){
    for(const d of dc.devices)if(id==='all'||d.id===id)invalidateDcRead(d.id);
    if(value){const snapshots=value as DcSnapshots;for(const row of snapshots.devices)battery(row);
      const lamp=snapshots.lamp, d=dc.devices.find(d=>d.id==='lamp');
      if(d){d.connected=lamp?.connection==='connected'?true:lamp?.connection==='disconnected'?false:null;d.stale=!!lamp?.stale;}
      dc.lamp=lamp&&!lamp.stale&&lamp.connection==='connected'&&typeof lamp.power==='boolean'&&typeof lamp.brightness==='number'&&typeof lamp.temperature==='number'?lamp as DcLampState:null;
    }else {for(const d of dc.devices)if(id==='all'||d.id===id){d.battery=null;delete dc.settings[d.id];delete dc.dpi[d.id];delete dc.lighting[d.id];}if(id==='all'||id==='lamp')dc.lamp=null;}
  }
  const key=`${id}:${method}`;dc.readbacks[key]=(dc.readbacks[key]??0)+1;
}
export async function refreshDc():Promise<void> {
  if(pending)return pending;
  if(dc.module?.state!=='running')return;
  const epoch=generation; dc.reading=true;dc.error='';
  const valid=()=>generation===epoch&&dc.module?.state==='running';
  const call=async<T>(method:DcMethod,params:object,accept:(v:T)=>void)=>{
    if(!capable(method))return;
    const id='deviceId' in params?String(params.deviceId):method.startsWith('lamp.')?'lamp':'all';const revision=revisions[id]??0;
    try{const v=await dcApi.call<T>(method,params);if(valid()&&revision===(revisions[id]??0))accept(v);}catch(e){if(valid()&&revision===(revisions[id]??0)){
      dc.error=String(e instanceof Error?e.message:e);
      if(method==='settings.read')delete dc.settings[id];else if(method==='mouse.dpi.read')delete dc.dpi[id];else if(method==='keyboard.lighting.read')delete dc.lighting[id];else if(method==='lamp.read')dc.lamp=null;else if(method==='lamp.config.read')dc.config=null;
      else if(method==='battery.read'){const d=dc.devices.find(d=>d.id===id);if(d)d.battery=null;}
    }}
  };
  pending=(async()=>{
    await call<DcHidDevice[]>('device.list',{},rows=>{
      const ids=new Set(rows.map(r=>r.id));
      dc.devices=dc.devices.map(d=>d.kind==='lamp'?d:{...d,connected:ids.has(d.id),battery:ids.has(d.id)?d.battery:null});
      for(const r of rows){const old=dc.devices.find(d=>d.id===r.id);if(old){old.connected=true;continue;}
        const name=r.product||r.manufacturer||r.id;const kind:DeviceKind=/kx75/i.test(name)?'kb':/atk|f1/i.test(name)?'mouse':'other';
        dc.devices.push({id:r.id,name,kind,connected:true,battery:null,stale:false});}
    });
    await call<DcLampConfig>('lamp.config.read',{},v=>{dc.config=v;if(!dc.devices.some(d=>d.id==='lamp'))dc.devices.push({id:'lamp',name:'小米挂灯 1S',kind:'lamp',connected:null,battery:null,stale:false});});
    if(!valid())return;
    setOrder([...dc.order,...dc.devices.map(d=>d.id).filter(id=>!dc.order.includes(id))]);
    await call<DcSnapshots>('device.snapshots',{},v=>{for(const row of v.devices)battery(row);if(v.lamp){const d=dc.devices.find(d=>d.id==='lamp');if(d){d.connected=v.lamp.connection==='connected';d.stale=!!v.lamp.stale;}}});
    await Promise.all(dc.devices.filter(d=>d.connected&&d.kind!=='lamp').map(async d=>{
      await call<DcBattery>('battery.read',{deviceId:d.id},battery);
      if(!d.connected)return;
      await call<DcSettings>('settings.read',{deviceId:d.id},v=>dc.settings[d.id]=v);
      if(d.kind==='mouse')await call<DcDpi>('mouse.dpi.read',{deviceId:d.id},v=>dc.dpi[d.id]=v);
      if(d.kind==='kb')await call<DcLighting>('keyboard.lighting.read',{deviceId:d.id},v=>dc.lighting[d.id]=v);
    }));
    if(dc.config?.configured)await call<DcLampState>('lamp.read',{},v=>{dc.lamp=v;const d=dc.devices.find(d=>d.id==='lamp');if(d){d.connected=true;d.stale=false;}});
  })().finally(()=>{pending=null;dc.reading=false;if(generation!==epoch&&dc.module?.state==='running')void refreshDc();});
  return pending;
}
export async function connectDc() {
  let disposed=false;const stops:(()=>void)[]=[];
  try { const saved=JSON.parse(localStorage.getItem(KEY)||'[]');if(Array.isArray(saved))dc.order=saved.filter(v=>typeof v==='string'); }catch{/* empty order */}
  try {
    stops.push(await listen<DcModuleStatus>('dc://module',e=>acceptModule(e.payload)));
    stops.push(await listen<DcReadback>('dc://readback',e=>acceptReadback(e.payload)));
    stops.push(await listen<DcEvent>('dc://event',e=>{
      const event=e.payload;if(dc.module?.state!=='running'||event.seq<=seq)return;
      if(seq&&event.seq!==seq+1)void refreshDc();seq=event.seq;
      if(event.name==='battery.updated')battery({...event.data as DcBattery,deviceId:event.deviceId??(event.data as DcBattery).deviceId});
      else if(event.name==='lamp.updated') {const row=event.data as Partial<DcLampState>&{stale?:boolean;connection?:string};const d=dc.devices.find(d=>d.id==='lamp');if(d){d.stale=!!row.stale;if(row.connection)d.connected=row.connection==='connected'?true:row.connection==='disconnected'?false:null;}if(row.stale||row.connection==='unknown'){dc.lamp=null;}else if(typeof row.power==='boolean'){dc.lamp=row as DcLampState;if(d){d.connected=true;d.stale=false;}}}
      else if(event.name==='device.changed'){const d=dc.devices.find(d=>d.id===event.deviceId);if(d){d.connected=!!(event.data as {connected:boolean}).connected;if(!d.connected){d.battery=null;delete dc.settings[d.id];delete dc.dpi[d.id];delete dc.lighting[d.id];}}void refreshDc();}
    }));
    const status=await dcApi.status();if(!disposed)acceptModule(status);
  }catch(e){dc.error=String(e instanceof Error?e.message:e);}
  return ()=>{disposed=true;generation++;stops.forEach(stop=>stop());};
}
