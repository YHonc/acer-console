#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use serde_json::{json, Value};
use std::sync::{Arc,Mutex};
use tauri::Manager;
mod tray;
mod system_commands;
mod plugin_commands;
mod dc_commands;
mod thermal_commands;
mod boot_logo_commands;
mod rules_runtime;
mod exit_restore;
mod exit_bridge;
#[tauri::command]
fn preview_theme(app:tauri::AppHandle,theme:acer_system::Theme)->Result<(),String>{
    let native=match theme{acer_system::Theme::System=>None,acer_system::Theme::Light=>Some(tauri::Theme::Light),acer_system::Theme::Dark=>Some(tauri::Theme::Dark)};
    for window in app.webview_windows().values(){window.set_theme(native).map_err(|e|e.to_string())?;}
    Ok(())
}
#[tauri::command]
fn get_startup()->Result<Value,String>{serde_json::to_value(acer_system::get_startup().map_err(|e|e.to_string())?).map_err(|e|e.to_string())}
#[tauri::command]
fn set_startup(enabled:bool)->Result<Value,String>{serde_json::to_value(acer_system::set_startup(enabled).map_err(|e|e.to_string())?).map_err(|e|e.to_string())}

struct Backend(Mutex<Option<acer_device::Device>>);
struct BrokerState(Arc<Mutex<Option<acer_broker::BrokerClient>>>);
#[tauri::command]
async fn authorize_device(app:tauri::AppHandle,state:tauri::State<'_,BrokerState>,rules:tauri::State<'_,rules_runtime::RulesState>)->Result<(),String>{
    let refresh=rules.refresh.clone();
    let engine=rules.engine.clone();
    let broker=state.0.clone();
    tauri::async_runtime::spawn_blocking(move|| {
        let mut slot=broker.lock().map_err(|e|e.to_string())?;
        if app.state::<thermal_commands::ThermalState>().owns_fans.load(std::sync::atomic::Ordering::SeqCst){
            if let Some(client)=slot.as_ref(){thermal_commands::stop_locked(&app,client)?;}else{return Err("温控会话状态未知，请先停止并确认恢复".into());}
        }
        *slot=None;
        *slot=Some(acer_broker::BrokerClient::launch_user_initiated().map_err(|e|e.to_string())?);
        engine.lock().map_err(|e|e.to_string())?.on_wake();
        refresh.store(true,std::sync::atomic::Ordering::Relaxed);
        Ok(())
    }).await.map_err(|e|e.to_string())?
}
fn device(state: &tauri::State<Backend>) -> Result<acer_device::Device, String> {
    let mut guard = state.0.lock().map_err(|e| e.to_string())?;
    if guard.is_none() { *guard = Some(acer_device::Device::connect().map_err(|e| e.to_string())?); }
    Ok(guard.as_ref().unwrap().clone())
}
#[tauri::command]
async fn get_snapshot(state: tauri::State<'_,Backend>,broker:tauri::State<'_,BrokerState>) -> Result<Value,String> {
    let local=device(&state)?;
    let broker=broker.0.clone();
    let snapshot=tauri::async_runtime::spawn_blocking(move||{
        let mut slot=broker.lock().map_err(|e|e.to_string())?;
        if slot.as_ref().is_some_and(|client| !client.is_alive().unwrap_or(false)) { *slot=None; }
        match slot.as_ref(){Some(client)=>client.snapshot(),None=>local.snapshot()}.map_err(|e|e.to_string())
    }).await.map_err(|e|e.to_string())??;
    let s=serde_json::to_value(snapshot).map_err(|e|e.to_string())?;
    let lighting=if let Some(brightness)=s["lighting"]["brightness"].as_u64(){
        let zone=&s["lighting"]["zones"][0];
        let zones=s["lighting"]["zones"].as_array().map(|v|v.iter().map(|z|format!("#{:02x}{:02x}{:02x}",z[0].as_u64().unwrap_or(0),z[1].as_u64().unwrap_or(0),z[2].as_u64().unwrap_or(0))).collect::<Vec<_>>());
        json!({"enabled":brightness>0,"brightness":brightness,"color":format!("#{:02x}{:02x}{:02x}",zone[0].as_u64().unwrap_or(0),zone[1].as_u64().unwrap_or(0),zone[2].as_u64().unwrap_or(0)),"zones":zones})
    }else{Value::Null};
    Ok(json!({"timestamp":s["captured_at_ms"].to_string(),"cpu_temperature":s["cpu_temperature_c"],"gpu_temperature":s["gpu_temperature_c"],"cpu_fan_rpm":s["cpu_fan_rpm"],"gpu_fan_rpm":s["gpu_fan_rpm"],"profile":s["profile"],"fan_mode":s["fan_mode"],"cpu_fan_percent":s["cpu_fan_percent"],"gpu_fan_percent":s["gpu_fan_percent"],"ac_power":s["ac_online"],"battery_percent":s["battery_percent"],"battery_care":s["battery_care"],"lighting":lighting,"lighting_effect":s["lighting_effect"],"lighting_config":lighting_config_json(&s["lighting_config"]),"errors":s["reads"].as_array().into_iter().flatten().filter_map(|r|r.get("error").filter(|v|!v.is_null()).map(|e|e["message"].as_str().unwrap_or("硬件读取失败").to_owned())).collect::<Vec<_>>()}))
}
#[tauri::command]
async fn get_capabilities(state: tauri::State<'_,Backend>,broker:tauri::State<'_,BrokerState>) -> Result<Value,String> {
    let local=device(&state)?;
    let broker=broker.0.clone();
    let caps=tauri::async_runtime::spawn_blocking(move||{
        let mut slot=broker.lock().map_err(|e|e.to_string())?;
        if slot.as_ref().is_some_and(|client| !client.is_alive().unwrap_or(false)) { *slot=None; }
        match slot.as_ref(){Some(client)=>client.capabilities(),None=>local.capabilities()}.map_err(|e|e.to_string())
    }).await.map_err(|e|e.to_string())??;
    let c=serde_json::to_value(caps).map_err(|e|e.to_string())?;
    let items=c["items"].as_array();
    let readable=items.is_some_and(|v|v.iter().any(|i|i["readable"]==true));
    let telemetry_blocked=items.is_some_and(|v|v.iter().any(|i|i["status"]=="permission_blocked"));
    let telemetry=json!({"supported":readable,"status":if readable{"verified"}else if telemetry_blocked{"permission_blocked"}else{"protocol_pending"},"reason":if readable{"设备状态已连接"}else if telemetry_blocked{"需要授权设备访问"}else{"尚未取得可用的设备状态"},"diagnostic_reason":c["items"].as_array().into_iter().flatten().filter_map(|i|i["reason"].as_str()).collect::<Vec<_>>().join("；")});
    let capability=|id:&str| {
        match items.and_then(|v|v.iter().find(|i|i["id"]==id)) {
            Some(item)=>{
                let reason=if item["writable"]==true{"设备已验证，应用后会读取确认"}else if item["status"]=="permission_blocked"{"需要授权设备访问"}else if id=="lighting_restore"{"本次连接尚无可恢复的灯光记录"}else if item["status"]=="verified"{"当前供电或设备状态不满足操作条件"}else if item["status"]=="hardware_unsupported"{"当前设备不支持此项功能"}else{"此项控制尚在验证，暂不可操作"};
                json!({"supported":item["writable"],"reason":reason,"diagnostic_reason":item["reason"],"status":item["status"]})
            },
            None=>json!({"supported":false,"reason":"协议待逆向","status":"protocol_pending"})
        }
    };
    Ok(json!({"model":c["model"],"bios_version":c["bios"],"telemetry":telemetry,"profiles":["quiet","balanced","performance","turbo"],"profile_control":capability("profile"),"fan_control":capability("fan_mode"),"lighting_control":capability("lighting"),"lighting_dynamic_control":capability("lighting_dynamic"),"lighting_restore_control":capability("lighting_restore"),"fan_custom_control":capability("fan_custom"),"battery_control":capability("battery_care"),"fan_percent_range":if c["model"]=="Predator PHN16-71" && c["bios"]=="V1.18" {json!({"min":0,"max":100})}else{Value::Null}}))
}
#[tauri::command]
async fn set_profile(app:tauri::AppHandle,state:tauri::State<'_,Backend>,broker:tauri::State<'_,BrokerState>,profile: acer_protocol::Profile) -> Result<Value,String> { apply(app,device(&state)?,broker.0.clone(),acer_protocol::Command::SetProfile{profile}).await }
#[tauri::command]
async fn set_fan_mode(app:tauri::AppHandle,state:tauri::State<'_,Backend>,broker:tauri::State<'_,BrokerState>,mode: acer_protocol::FanMode) -> Result<Value,String> { apply(app,device(&state)?,broker.0.clone(),acer_protocol::Command::SetFanMode{mode}).await }
#[tauri::command]
async fn set_fan_speeds(app:tauri::AppHandle,state:tauri::State<'_,Backend>,broker:tauri::State<'_,BrokerState>,cpu_percent:u8,gpu_percent:u8)->Result<Value,String>{
    if cpu_percent>100 || gpu_percent>100{return Err("风扇目标不能超过100%".into())}
    apply(app,device(&state)?,broker.0.clone(),acer_protocol::Command::SetFanSpeeds{cpu_percent,gpu_percent}).await
}
#[tauri::command]
async fn set_battery_care(app:tauri::AppHandle,state:tauri::State<'_,Backend>,broker:tauri::State<'_,BrokerState>,enabled:bool)->Result<Value,String>{
    apply(app,device(&state)?,broker.0.clone(),acer_protocol::Command::SetBatteryCare{enabled}).await
}
#[tauri::command]
async fn set_lighting(app:tauri::AppHandle,state:tauri::State<'_,Backend>,broker:tauri::State<'_,BrokerState>,lighting: Value) -> Result<Value,String> {
    let enabled=lighting["enabled"].as_bool().ok_or("灯光开关无效")?;
    let brightness=lighting["brightness"].as_u64().filter(|v|*v<=100).ok_or("亮度应在0至100之间")? as u8;
    let zones=if let Some(values)=lighting.get("zones"){
        let values=values.as_array().filter(|v|v.len()==4).ok_or("必须提供四个分区颜色")?;
        let mut zones=[[0u8;3];4];for (index,value) in values.iter().enumerate(){zones[index]=parse_rgb(value.as_str().ok_or("颜色无效")?)?;}zones
    }else{[parse_rgb(lighting["color"].as_str().ok_or("颜色无效")?)?;4]};
    apply(app,device(&state)?,broker.0.clone(),acer_protocol::Command::SetLighting{lighting:acer_protocol::Lighting{brightness:if enabled{brightness}else{0},zones}}).await
}
fn parse_rgb(color:&str)->Result<[u8;3],String>{
    if color.len()!=7 || !color.starts_with('#') || !color[1..].bytes().all(|c|c.is_ascii_hexdigit()){return Err("颜色必须是#RRGGBB".into())}
    Ok([u8::from_str_radix(&color[1..3],16).map_err(|e|e.to_string())?,u8::from_str_radix(&color[3..5],16).map_err(|e|e.to_string())?,u8::from_str_radix(&color[5..7],16).map_err(|e|e.to_string())?])
}
#[tauri::command]
async fn set_lighting_animation(app:tauri::AppHandle,state:tauri::State<'_,Backend>,broker:tauri::State<'_,BrokerState>,lighting:Value)->Result<Value,String>{
    let effect=serde_json::from_value(lighting["effect"].clone()).map_err(|_|"灯效类型无效")?;
    let byte=|key:&str|lighting[key].as_u64().filter(|v|*v<=255).map(|v|v as u8).ok_or_else(||format!("灯效参数{key}无效"));
    let color=parse_rgb(lighting["color"].as_str().ok_or("灯效颜色无效")?)?;
    let config=acer_protocol::LightingAnimation{effect,speed:byte("speed")?,brightness:byte("brightness")?,direction:byte("direction")?,color};
    apply(app,device(&state)?,broker.0.clone(),acer_protocol::Command::SetLightingAnimation{lighting:config}).await
}
#[tauri::command]
async fn restore_lighting(app:tauri::AppHandle,state:tauri::State<'_,Backend>,broker:tauri::State<'_,BrokerState>)->Result<Value,String>{
    apply(app,device(&state)?,broker.0.clone(),acer_protocol::Command::RestoreLighting{}).await
}
fn rgb_json(value:&Value)->Value{
    let Some(parts)=value.as_array().filter(|parts|parts.len()==3) else{return Value::Null};
    let Some(rgb)=parts.iter().map(|n|n.as_u64().filter(|n|*n<=255)).collect::<Option<Vec<_>>>() else{return Value::Null};
    json!(format!("#{:02x}{:02x}{:02x}",rgb[0],rgb[1],rgb[2]))
}
fn lighting_config_json(config:&Value)->Value{
    if !config.is_object(){return Value::Null}
    let zones=config["zones"].as_array().map(|zones|zones.iter().map(rgb_json).collect::<Vec<_>>());
    json!({"effect":config["effect"],"speed":config["speed"],"brightness":config["brightness"],"direction":config["direction"],"color":rgb_json(&config["color"]),"zones":zones})
}
async fn apply(app:tauri::AppHandle,local:acer_device::Device,broker:Arc<Mutex<Option<acer_broker::BrokerClient>>>,command:acer_protocol::Command)->Result<Value,String>{
    let exit=app.state::<exit_bridge::ExitCoordinator>().inner().clone();
    let report=tauri::async_runtime::spawn_blocking(move||{
        let slot=broker.lock().map_err(|e|e.to_string())?;
        if exit.gate.load(std::sync::atomic::Ordering::SeqCst){return Err("程序正在退出，请等待恢复结果".into())}
        let report=match slot.as_ref(){Some(client)=>client.apply(command.clone()),None=>local.apply(command.clone())}.map_err(|e|e.to_string())?;
        exit.state.record(&command,&report)?;
        Ok::<_,String>(report)
    }).await.map_err(|e|e.to_string())??;
    let status=match report.status {acer_protocol::ApplyStatus::Applied=>"applied",acer_protocol::ApplyStatus::Unconfirmed=>"unconfirmed",_=>"failed"};
    let message=match report.status{acer_protocol::ApplyStatus::Applied=>"设置已应用，设备读回已确认",acer_protocol::ApplyStatus::Failed=>"设备未接受设置，请检查诊断记录",acer_protocol::ApplyStatus::Rejected=>"当前条件不允许此操作，设备设置未改变",acer_protocol::ApplyStatus::Unconfirmed=>"请求已发送，但设备状态尚未确认，请刷新检查"};
    Ok(json!({"status":status,"message":message,"diagnostic":report.message}))
}
#[tauri::command]
fn get_settings(app:tauri::AppHandle)->Result<Value,String>{
    let dir=app.path().app_config_dir().map_err(|e|e.to_string())?;
    serde_json::to_value(acer_system::read_settings(&dir).map_err(|e|e.to_string())?).map_err(|e|e.to_string())
}
#[tauri::command]
async fn save_settings(app:tauri::AppHandle,settings:Value)->Result<Value,String>{
    tauri::async_runtime::spawn_blocking(move||{
    let dir=app.path().app_config_dir().map_err(|e|e.to_string())?;
    let settings:acer_system::Settings=serde_json::from_value(settings).map_err(|e|e.to_string())?;
    let broker=app.state::<BrokerState>();let slot=broker.0.lock().map_err(|e|e.to_string())?;
    if app.state::<exit_bridge::ExitCoordinator>().gate.load(std::sync::atomic::Ordering::SeqCst){return Err("程序正在退出，暂不能保存设置".into())}
    let current=if settings.restore_on_exit{slot.as_ref().and_then(|client|client.snapshot().ok())}else{None};
    app.state::<exit_bridge::ExitCoordinator>().state.set_enabled(settings.restore_on_exit,current.as_ref())?;
    serde_json::to_value(acer_system::write_settings(&dir,&settings).map_err(|e|e.to_string())?).map_err(|e|e.to_string())
    }).await.map_err(|e|e.to_string())?
}
fn main(){
    let start_tray=std::env::args().any(|arg|arg=="--tray");
    #[cfg(windows)] unsafe {
        #[link(name="shell32")] unsafe extern "system" {fn SetCurrentProcessExplicitAppUserModelID(id:*const u16)->i32;}
        let id:Vec<u16>="local.phn16.acerconsole".encode_utf16().chain(Some(0)).collect();
        SetCurrentProcessExplicitAppUserModelID(id.as_ptr());
    }
    tauri::Builder::default().manage(Backend(Mutex::new(None)))
      .plugin(tauri_plugin_dialog::init())
      .plugin(tauri_plugin_single_instance::init(|app,args,_cwd| {if !args.iter().any(|arg|arg=="--tray"){tray::open(app);}}))
      .manage(BrokerState(Arc::new(Mutex::new(None))))
      .manage(system_commands::OfficialTaskLock(Arc::new(Mutex::new(()))))
      .manage(rules_runtime::RulesState::default())
      .manage(exit_bridge::ExitCoordinator::default())
      .manage(thermal_commands::ThermalState::default())
      .setup(move|app| {dc_commands::setup(app.handle())?;tray::setup(app)?;rules_runtime::start(app.handle().clone());thermal_commands::start_monitor(app.handle().clone()); if start_tray {if let Some(window)=app.get_webview_window("main"){window.destroy()?;}} Ok(())})
      .invoke_handler(tauri::generate_handler![dc_commands::dc_module_status,dc_commands::dc_module_trust_and_start,dc_commands::dc_module_set_keep,dc_commands::dc_call,thermal_commands::get_thermal_config,thermal_commands::save_thermal_config,thermal_commands::get_thermal_status,thermal_commands::start_thermal,thermal_commands::stop_thermal,plugin_commands::get_device_center_plugin,plugin_commands::bind_device_center_plugin,plugin_commands::bind_device_center_module,plugin_commands::device_center_action,plugin_commands::get_device_center_management_log,plugin_commands::get_central_plugin,plugin_commands::set_central_guard,plugin_commands::get_central_log,boot_logo_commands::get_boot_logo,boot_logo_commands::choose_boot_logo,boot_logo_commands::prepare_boot_logo_reset,boot_logo_commands::prepare_boot_logo_restore,boot_logo_commands::commit_boot_logo,boot_logo_commands::discard_boot_logo,system_commands::get_audio,system_commands::set_audio,system_commands::get_usb_charge,system_commands::set_usb_charge,set_lighting_animation,restore_lighting,system_commands::get_vendor_controls,system_commands::set_vendor_control,preview_theme,exit_bridge::get_exit_status,exit_bridge::exit_without_restore,rules_runtime::get_rules,rules_runtime::save_rules,system_commands::official_task,get_startup,set_startup,authorize_device,get_snapshot,get_capabilities,set_profile,set_fan_mode,set_fan_speeds,set_battery_care,set_lighting,get_settings,save_settings])
      .build(tauri::generate_context!()).expect("启动桌面程序失败")
      .run(move|app,event| {
        if matches!(event,tauri::RunEvent::Exit){app.state::<rules_runtime::RulesState>().stop.store(true,std::sync::atomic::Ordering::Relaxed);}
        if let tauri::RunEvent::ExitRequested{code,api,..}=event {
            let settings=get_settings(app.clone()).ok();
            if code.is_none()&&(start_tray || settings.as_ref().and_then(|v|v["close_to_tray"].as_bool()).unwrap_or(false)) {api.prevent_exit();return}
            if exit_bridge::attempt(app){api.prevent_exit();}
        }
      });
}
