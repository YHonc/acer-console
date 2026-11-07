use std::sync::{Arc,Mutex};
use tauri::Manager;
pub struct OfficialTaskLock(pub Arc<Mutex<()>>);
#[tauri::command]
pub async fn get_usb_charge(broker:tauri::State<'_,super::BrokerState>)->Result<serde_json::Value,String>{
    let broker=broker.0.clone();
    tauri::async_runtime::spawn_blocking(move||{
        let slot=broker.lock().map_err(|e|e.to_string())?;
        slot.as_ref().ok_or("请先授权设备连接，再读取 USB 关机充电状态")?.get_usb_charge().map_err(|e|e.to_string())
    }).await.map_err(|e|e.to_string())?
}
#[tauri::command]
pub async fn set_usb_charge(app:tauri::AppHandle,broker:tauri::State<'_,super::BrokerState>,state:acer_broker::UsbState)->Result<serde_json::Value,String>{
    let broker=broker.0.clone();let gate=app.state::<super::exit_bridge::ExitCoordinator>().gate.clone();
    tauri::async_runtime::spawn_blocking(move||{
        let slot=broker.lock().map_err(|e|e.to_string())?;
        if gate.load(std::sync::atomic::Ordering::SeqCst){return Err("程序正在退出，请稍后重试".into())}
        slot.as_ref().ok_or("请先授权设备连接，再修改 USB 关机充电设置")?.set_usb_charge(state).map_err(|e|e.to_string())
    }).await.map_err(|e|e.to_string())?
}
#[tauri::command]
pub async fn get_vendor_controls(broker:tauri::State<'_,super::BrokerState>)->Result<serde_json::Value,String>{
    let broker=broker.0.clone();
    tauri::async_runtime::spawn_blocking(move||{
        let slot=broker.lock().map_err(|e|e.to_string())?;
        let vendor = slot.as_ref().ok_or_else(||"请先授权设备连接".to_string())
            .and_then(|client|client.vendor_controls().map_err(|e|e.to_string()));
        let mut value = match vendor {
            Ok(value) => value,
            Err(message) => serde_json::json!({"items":[
                {"setting":"lcd_overdrive","enabled":null,"writable":false,"error":message},
                {"setting":"boot_sound","enabled":null,"writable":false,"error":message},
                {"setting":"backlight_timeout","enabled":null,"writable":false,"error":message}
            ]}),
        };
        let items=value["items"].as_array_mut().ok_or("设备设置列表格式错误")?;
        items.retain(|item|item["setting"]!="sticky_key");
        items.push(match acer_system::sticky_keys::get() {
            Ok(state)=>serde_json::json!({"setting":"sticky_key","enabled":state.enabled,"writable":state.available,"error":null,"raw":state,"backend":"direct_win32"}),
            Err(e)=>serde_json::json!({"setting":"sticky_key","enabled":null,"writable":false,"error":e.message,"backend":"direct_win32"}),
        });
        value["backend"]=serde_json::json!("mixed");
        value["dependency"]=serde_json::json!("粘滞键控制当前 Windows 用户；其余设置通过本机硬件接口直接控制。");
        Ok(value)
    }).await.map_err(|e|e.to_string())?
}
#[tauri::command]
pub async fn set_vendor_control(app:tauri::AppHandle,broker:tauri::State<'_,super::BrokerState>,setting:acer_broker::VendorSetting,enabled:bool)->Result<serde_json::Value,String>{
    let broker=broker.0.clone();let gate=app.state::<super::exit_bridge::ExitCoordinator>().gate.clone();
    tauri::async_runtime::spawn_blocking(move||{
        let slot=broker.lock().map_err(|e|e.to_string())?;
        if gate.load(std::sync::atomic::Ordering::SeqCst){return Err("程序正在退出，请稍后重试".into())}
        if setting==acer_broker::VendorSetting::StickyKey {
            let after=acer_system::sticky_keys::set(enabled).map_err(|e|e.to_string())?;
            return Ok(serde_json::json!({"status":"applied","setting":setting,"enabled":after.enabled,"message":"当前用户的粘滞键设置已应用，Windows 读回一致","after":after,"backend":"direct_win32"}));
        }
        slot.as_ref().ok_or("请先授权设备连接")?.set_vendor_control(setting,enabled).map_err(|e|e.to_string())
    }).await.map_err(|e|e.to_string())?
}
use acer_broker::OfficialAction;
#[tauri::command]
pub async fn official_task(app:tauri::AppHandle,state:tauri::State<'_,OfficialTaskLock>,action:OfficialAction)->Result<serde_json::Value,String>{
    let path=app.path().app_config_dir().map_err(|e|e.to_string())?;
    let lock=state.0.clone();
    let broker=app.state::<super::BrokerState>().0.clone();
    let exit_gate=app.state::<super::exit_bridge::ExitCoordinator>().gate.clone();
    tauri::async_runtime::spawn_blocking(move||{
        let _guard=lock.lock().map_err(|e|e.to_string())?;
        if action!=OfficialAction::Query {
            let slot=broker.lock().map_err(|e|e.to_string())?;
            if exit_gate.load(std::sync::atomic::Ordering::SeqCst){return Err("正在退出，请稍后重试".into())}
            let client=slot.as_ref().ok_or("请先授权设备连接，再管理官方启动项")?;
            let response=client.official_task(action).map_err(|e|e.to_string())?;
            if let Some(message)=response.get("error").and_then(|v|v.as_str()) {return Err(message.to_owned())}
            return Ok(response);
        }
        let mut state=serde_json::to_value(acer_system::get_official_task().map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
        let backup=acer_system::get_official_backup(&path);
        state["backup_valid"]=serde_json::json!(backup.backup_valid);
        state["backup_original_enabled"]=serde_json::json!(backup.backup_original_enabled);
        state["backup_error"]=serde_json::json!(backup.backup_error);
        Ok(state)
    }).await.map_err(|e|e.to_string())?
}
#[tauri::command]
pub async fn get_audio(broker:tauri::State<'_,super::BrokerState>)->Result<serde_json::Value,String>{
    let broker=broker.0.clone();
    tauri::async_runtime::spawn_blocking(move||{
        let slot=broker.lock().map_err(|e|e.to_string())?;
        slot.as_ref().ok_or("请先授权设备连接，再读取音效状态")?.get_audio().map_err(|e|e.to_string())
    }).await.map_err(|e|e.to_string())?
}
#[tauri::command]
pub async fn set_audio(app:tauri::AppHandle,broker:tauri::State<'_,super::BrokerState>,preset:acer_broker::audio_controls::AudioPreset)->Result<serde_json::Value,String>{
    let broker=broker.0.clone();let gate=app.state::<super::exit_bridge::ExitCoordinator>().gate.clone();
    tauri::async_runtime::spawn_blocking(move||{
        let slot=broker.lock().map_err(|e|e.to_string())?;
        if gate.load(std::sync::atomic::Ordering::SeqCst){return Err("程序正在退出，请稍后重试".into())}
        slot.as_ref().ok_or("请先授权设备连接，再修改音效")?.set_audio(preset).map_err(|e|e.to_string())
    }).await.map_err(|e|e.to_string())?
}
