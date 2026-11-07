use tauri::Manager;
use acer_system::central_plugin::{GuardAction,PluginLog};
use acer_system::device_center_plugin::{self as dc,DeviceCenterAction};
use tauri_plugin_dialog::{DialogExt,FilePath};

#[tauri::command]
pub async fn get_device_center_plugin(app:tauri::AppHandle)->Result<serde_json::Value,String>{
    let dir=app.path().app_config_dir().map_err(|e|e.to_string())?;
    tauri::async_runtime::spawn_blocking(move||dc::query(&dir).map_err(|e|e.to_string())).await.map_err(|e|e.to_string())?
}
#[tauri::command]
pub async fn bind_device_center_plugin(app:tauri::AppHandle)->Result<Option<serde_json::Value>,String>{
    tauri::async_runtime::spawn_blocking(move||{
        let Some(file)=app.dialog().file().set_title("选择已安装的Device Center主程序（不要选择setup）").add_filter("Windows程序",&["exe"]).blocking_pick_file()else{return Ok(None)};
        let FilePath::Path(path)=file else{return Err("请选择本机主程序".into())};
        let dir=app.path().app_config_dir().map_err(|e|e.to_string())?;
        dc::bind(&dir,&path).map(Some).map_err(|e|e.to_string())
    }).await.map_err(|e|e.to_string())?
}
#[tauri::command]
pub async fn bind_device_center_module(app:tauri::AppHandle)->Result<Option<serde_json::Value>,String>{
    tauri::async_runtime::spawn_blocking(move||{
        let Some(folder)=app.dialog().file().set_title("选择 Device Center 模块文件夹").blocking_pick_folder()else{return Ok(None)};
        let FilePath::Path(path)=folder else{return Err("请选择本机模块文件夹".into())};
        let dir=app.path().app_config_dir().map_err(|e|e.to_string())?;
        dc::bind_module(&dir,&path).map(Some).map_err(|e|e.to_string())
    }).await.map_err(|e|e.to_string())?
}
#[tauri::command]
pub async fn device_center_action(app:tauri::AppHandle,action:DeviceCenterAction)->Result<serde_json::Value,String>{
    let dir=app.path().app_config_dir().map_err(|e|e.to_string())?;
    let gate=app.state::<super::exit_bridge::ExitCoordinator>().gate.clone();
    tauri::async_runtime::spawn_blocking(move||{
        if gate.load(std::sync::atomic::Ordering::SeqCst){return Err("控制台正在退出".into())}
        dc::action(&dir,action).map_err(|e|e.to_string())
    }).await.map_err(|e|e.to_string())?
}
#[tauri::command]
pub async fn get_device_center_management_log(app:tauri::AppHandle)->Result<serde_json::Value,String>{
    let dir=app.path().app_config_dir().map_err(|e|e.to_string())?;
    tauri::async_runtime::spawn_blocking(move||dc::log(&dir).map_err(|e|e.to_string())).await.map_err(|e|e.to_string())?
}

#[tauri::command]
pub async fn get_central_plugin(broker:tauri::State<'_,super::BrokerState>)->Result<serde_json::Value,String>{
    let broker=broker.0.clone();
    tauri::async_runtime::spawn_blocking(move||{
        let slot=broker.lock().map_err(|e|e.to_string())?;
        match slot.as_ref(){Some(client)=>client.central_plugin().map_err(|e|e.to_string()),None=>acer_system::central_plugin::query().map_err(|e|e.to_string())}
    }).await.map_err(|e|e.to_string())?
}
#[tauri::command]
pub async fn set_central_guard(app:tauri::AppHandle,action:GuardAction)->Result<serde_json::Value,String>{
    let broker=app.state::<super::BrokerState>().0.clone();
    let gate=app.state::<super::exit_bridge::ExitCoordinator>().gate.clone();
    tauri::async_runtime::spawn_blocking(move||{
        let slot=broker.lock().map_err(|e|e.to_string())?;
        if gate.load(std::sync::atomic::Ordering::SeqCst){return Err("控制台正在退出".into())}
        slot.as_ref().ok_or("请先授权设备连接，再启用或暂停看门狗")?.set_central_guard(action).map_err(|e|e.to_string())
    }).await.map_err(|e|e.to_string())?
}
#[tauri::command]
pub async fn get_central_log(log:PluginLog)->Result<serde_json::Value,String>{
    tauri::async_runtime::spawn_blocking(move||acer_system::central_plugin::read_log(log).map_err(|e|e.to_string())).await.map_err(|e|e.to_string())?
}
