// Browser-only transport fixture. Nothing is forwarded to a backend or hardware.
(() => {
  const cap = { supported: true, status: 'verified' };
  const profiles = ['quiet', 'balanced', 'performance', 'turbo', 'eco'];
  const snapshot = {timestamp: String(Date.now()), profile: 'balanced', fan_mode: 'auto', cpu_fan_percent: null, gpu_fan_percent: null,
    cpu_temperature: null, gpu_temperature: null, cpu_fan_rpm: null, gpu_fan_rpm: null, ac_power: true,
    battery_percent: null, battery_care: false, lighting: null,
    lighting_config: {effect: 'static', speed: 5, brightness: 40, direction: 1, color: '#ffffff', zones: Array(4).fill('#ffffff')}, errors: []};
  const settings = {theme: 'light', poll_interval_ms: 60000, close_to_tray: false, restore_on_exit: false};
  const rules = {config: {enabled: false, ac_profile: 'balanced', dc_profile: null}, status: '', ac_profiles: profiles, dc_profiles: ['quiet']};
  const vendor = {backend: 'mixed', dependency: 'Browser fixture', items: ['lcd_overdrive', 'boot_sound', 'sticky_key', 'backlight_timeout'].map(setting => ({setting, enabled: false, writable: true, error: null, raw: null}))};
  const audio = {backend: 'official_service', dependency: '', supported: ['music', 'movie', 'voice'], preset: 'music', availability: 'available', writable: true, error: null};
  const usb = {backend: 'direct_wmi', state: {enabled: false, threshold: 10}, writable: true, ack: 'confirmed', error: null, raw: null};
  const official = {task_name: 'Browser fixture', enabled: true, definition_sha256: 'a'.repeat(64), backup_valid: true, backup_original_enabled: true, backup_error: null};
  const central = {captured_at_ms: Date.now(), logical_processors: 1, service: null, metrics: null,
    guard: {installed: true, enabled: true, state: 3, verified: true, threshold: 100, cooldown_seconds: 60, last_run: null, next_run: null, last_result: null, error: null}, errors: []};
  const dc = {configured: true, executable: 'fixture.exe', binding_valid: true, binding_error: null, captured_at_ms: Date.now(), logical_processors: 1, running: false, processes: [], startup: {present: false, enabled: false, mismatch: false, other_entries: []}};
  const clone = value => structuredClone(value);
  window.syncFixture = {snapshot, settings, rules, vendor, audio, usb, central, dc, calls: [], delay: 0, fail: [], holds: {}, releases: {}};
  const methods = ['device.list','device.snapshots','battery.read','settings.read','settings.apply','mouse.dpi.read','mouse.dpi.apply','mouse.mapping.read','mouse.mapping.apply','keyboard.lighting.read','keyboard.lighting.apply','lamp.config.read','lamp.config.save','lamp.read','lamp.apply'];
  const module = {state:'running',pid:8840,protocol:{major:1,minor:0},moduleVersion:'0.1.1',capabilities:methods,keep:false,message:null,reconnectAttempt:2,exitCode:10};
  const field = (key,value,options=null) => ({key,label:key,kind:options?'select':'number',value,available:true,reason:null,min:null,max:null,step:null,options:options?.map(v=>({value:v,label:String(v)}))??null});
  const deviceSettings = {
    mouse:{deviceId:'mouse',model:'ATK F1',stateToken:'mouse-token',sampledAt:1,verifiedOnHardware:false,fields:[field('reportRateHz',1000,[125,500,1000,2000]),field('dpiLevel',2,[0,1,2,3,4])]},
    kb:{deviceId:'kb',model:'KX75',stateToken:'kb-token',sampledAt:1,verifiedOnHardware:false,fields:[field('reportRateHz',1000,[125,250,500,1000]),field('sleepSeconds',300,[60,300,600,1800,0]),field('lowLatency',1),field('macMode',0),field('winLock',null)]}
  };
  const dpi = {deviceId:'mouse',model:'ATK F1',sensor:'fixture',stateToken:'dpi-token',currentLevel:2,levelCount:5,levels:[400,800,1600,3200,6400].map((v,i)=>({level:i,xDpi:v,yDpi:v,editable:true})),min:50,max:26000,stepSegments:[{until:26000,step:50}],verifiedOnHardware:false};
  const lighting = {deviceId:'kb',model:'KX75',sampledAt:1,stateToken:'light-token',mode:11,brightness:3,speed:3,multiColor:true,rgb:[10,132,255],colorError:null,modes:[{id:1,label:'常亮模式'},{id:2,label:'呼吸模式'},{id:11,label:'随波逐流'},{id:3,label:'梦幻彩虹'},{id:7,label:'涟漪扩散'},{id:0,label:'关闭'}].map(v=>({...v,brightnessEditable:v.id!==0,speedEditable:![0,1].includes(v.id),colorEditable:![0,3].includes(v.id)})),unsupported:[],verifiedOnHardware:false};
  const lamp = {power:true,brightness:60,temperature:4000,sampledAt:1};
  const config = {configured:true,ip:'192.168.1.50',tokenTail:'7c2e'};
  const hid = ['mouse','kb'].map((id,i)=>({id,vendorId:i?0x3554:0x3554,productId:i?0xfa09:1,usagePage:1,usage:2,interfaceNumber:0,manufacturer:null,product:i?'KX75 键盘':'ATK F1 鼠标',batterySupported:true}));
  const disconnected = [];
  const callbacks = new Map(), listeners = new Map(); let callbackId=0;
  Object.assign(window.syncFixture,{dcModule:module,dcData:{settings:deviceSettings,dpi,lighting,lamp,config,hid,disconnected},dcFailure:null});
  window.syncFixture.emit=(name,payload)=>{for(const [id,l] of listeners)if(l.event===name)callbacks.get(l.handler)?.({event:name,id,payload:clone(payload)});};
  window.syncFixture.setDcState=state=>{module.state=state;window.syncFixture.emit('dc://module',module);};
  window.syncFixture.readback=(method,deviceId,result,error=null)=>window.syncFixture.emit('dc://readback',{method,deviceId,...(error?{ok:false,error}:{ok:true,result})});
  window.isTauri = true;
  window.__TAURI_INTERNALS__ = {
    metadata: {currentWindow: {label: 'main'}, currentWebview: {label: 'main'}},
    transformCallback: callback => {const id=++callbackId;callbacks.set(id,callback);return id;}, unregisterCallback: id => callbacks.delete(id),
    invoke: async (command, args = {}) => {
      const f = window.syncFixture;
      f.calls.push({command, args: clone(args)});
      if(command==='plugin:event|listen'){const id=++callbackId;listeners.set(id,args);return id;}
      if(command==='plugin:event|unlisten'){listeners.delete(args.eventId);return;}
      if(command==='dc_module_status')return clone(module);
      if(command==='dc_module_trust_and_start'){module.state='running';f.emit('dc://module',module);return clone(module);}
      if(command==='dc_module_set_keep'){module.keep=args.keep;f.emit('dc://module',module);return clone(module);}
      if(command==='exit_without_restore'){f.exitRequested=true;return;}
      if(command==='dc_call'){
        const {method,params:q}=args;
        const get=()=>{switch(method){
          case 'device.list':return hid.filter(d=>!disconnected.includes(d.id));
          case 'device.snapshots':return {devices:hid.map(d=>({deviceId:d.id,batteryPercent:d.id==='mouse'?72:null,connection:disconnected.includes(d.id)?'disconnected':'connected',stale:false})),lamp:{...lamp,connection:'connected',stale:false}};
          case 'battery.read':return {deviceId:q.deviceId,batteryPercent:q.deviceId==='mouse'?72:null,connection:'connected',stale:false};
          case 'settings.read':return deviceSettings[q.deviceId]??{deviceId:q.deviceId,model:'New device',sampledAt:1,verifiedOnHardware:false,stateToken:null,fields:[]};
          case 'mouse.dpi.read':return dpi;
          case 'keyboard.lighting.read':return lighting;
          case 'lamp.read':return lamp;
          case 'lamp.config.read':return config;
          case 'mouse.mapping.read':return {deviceId:q.deviceId,buttons:[],stateToken:'mapping'};
        }};
        const captured=clone(get());
        if(f.holds[method])await new Promise(resolve=>(f.releases[method]??=[]).push(resolve));
        else if(f.delay)await new Promise(resolve=>setTimeout(resolve,f.delay));
        if(f.dcFailure&&method.endsWith('.apply')){
          if(f.dcRecoveryBeforeReturn){const r=f.dcRecoveryBeforeReturn;f.dcRecoveryBeforeReturn=null;f.readback(r.method,r.deviceId,r.result,r.error);}
          return {ok:false,error:clone(f.dcFailure)};
        }
        if(captured!==undefined)return {ok:true,result:captured};
        let result;
        if(method==='settings.apply'){const s=deviceSettings[q.deviceId];for(const field of s.fields)if(q.change[field.key]!=null)field.value=typeof q.change[field.key]==='boolean'?Number(q.change[field.key]):q.change[field.key];s.stateToken+='x';result=s;}
        else if(method==='mouse.dpi.apply'){dpi.currentLevel=q.change.level;result=dpi;}
        else if(method==='keyboard.lighting.apply'){Object.assign(lighting,q.change);result=lighting;}
        else if(method==='lamp.apply'){Object.assign(lamp,q.change);result=lamp;}
        else if(method==='lamp.config.save'){config.ip=q.ip;if(q.token)config.tokenTail=q.token.slice(-4);result=config;}
        else throw Error('Unimplemented fixture DC method: '+method);
        return {ok:true,result:clone(result)};
      }
      // Capture reads at invocation, allowing deterministic out-of-order responses.
      const reads = {get_snapshot: snapshot, get_settings: settings, get_rules: rules, get_startup: {enabled: false, entry_present: false, mismatch: false},
        get_capabilities: {model: 'Browser fixture', bios_version: null, telemetry: cap, profiles, profile_control: cap, fan_control: cap, fan_custom_control: cap, fan_percent_range: {min: 20, max: 100}, lighting_control: cap, lighting_dynamic_control: cap, lighting_restore_control: cap, battery_control: cap},
        get_audio: audio, get_usb_charge: usb, get_vendor_controls: vendor, get_central_plugin: central, get_device_center_plugin: dc,
        get_thermal_config: null, get_thermal_status: {active: false, restore_pending: false, message: '', cpu_target: null, gpu_target: null, last_report: null}, get_exit_status: {error: null}};
      const captured = clone(reads[command]);
      if (f.holds[command]) await new Promise(resolve => { (f.releases[command] ??= []).push(resolve); });
      else if (f.delay && (Object.hasOwn(reads, command) || command === 'official_task')) await new Promise(resolve => setTimeout(resolve, f.delay));
      if (f.fail.includes(command)) throw new Error('Fixture read failure');
      if (Object.hasOwn(reads, command)) return captured;
      if (command === 'save_settings') { Object.assign(settings, args.settings); return clone(settings); }
      if (command === 'save_rules') { rules.config = clone(args.config); return; }
      if (command === 'set_profile') snapshot.profile = args.profile;
      else if (command === 'set_battery_care') snapshot.battery_care = args.enabled;
      else if (command === 'set_fan_mode') snapshot.fan_mode = args.mode;
      else if (command === 'set_fan_speeds') Object.assign(snapshot, {fan_mode: 'custom', cpu_fan_percent: args.cpuPercent, gpu_fan_percent: args.gpuPercent});
      else if (command === 'set_audio') audio.preset = args.preset;
      else if (command === 'set_usb_charge') usb.state = clone(args.state);
      else if (command === 'set_vendor_control') vendor.items.find(item => item.setting === args.setting).enabled = args.enabled;
      else if (command === 'official_task') return clone(official);
      else if (command === 'set_central_guard') { central.guard.enabled = args.action === 'enable'; return clone(central); }
      else if (command === 'bind_device_center_module' || command === 'bind_device_center_plugin') return clone(dc);
      else if (command === 'device_center_action') return {message: 'Browser fixture'};
      else if (command.startsWith('plugin:') || command === 'preview_theme') return null;
      else throw new Error('Unimplemented fixture command: ' + command);
      return {status: 'applied', message: 'Browser fixture'};
    }
  };
})();
