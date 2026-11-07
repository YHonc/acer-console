use acer_system::{device_center_ipc::{CallResult,DcError}, device_center_runtime::Host,module_discovery::ModuleStatus};
use std::sync::{Arc,Mutex,atomic::Ordering};
use tauri::{Emitter,Manager};

pub struct DcState(pub Arc<Mutex<Host>>);
pub fn setup(app:&tauri::AppHandle)->Result<(),Box<dyn std::error::Error>>{
    let config=app.path().app_config_dir()?;
    let exe=std::env::current_exe()?;
    let root=exe.parent().ok_or("无法定位程序目录")?.join("modules/device-center");
    let handle=app.clone();
    let host=Arc::new(Mutex::new(Host::new(root,config,Arc::new(move|name,value|{let _=handle.emit(name,value);})).map_err(std::io::Error::other)?));
    app.manage(DcState(host.clone()));
    let gate=app.state::<super::exit_bridge::ExitCoordinator>().gate.clone();
    std::thread::Builder::new().name("dc-module-watch".into()).spawn(move||loop{
        match host.lock(){Ok(mut host)=>{if host.is_exiting(){break}if !gate.load(Ordering::SeqCst){host.tick()}},Err(_)=>break}
        std::thread::sleep(std::time::Duration::from_millis(100));
    })?;
    Ok(())
}
fn host(app:&tauri::AppHandle)->Arc<Mutex<Host>>{app.state::<DcState>().0.clone()}
fn active(app:&tauri::AppHandle)->Result<(),String>{if app.state::<super::exit_bridge::ExitCoordinator>().gate.load(Ordering::SeqCst){Err("控制台正在退出".into())}else{Ok(())}}
#[tauri::command]
pub async fn dc_module_status(app:tauri::AppHandle)->Result<ModuleStatus,String>{
    let host=host(&app);tauri::async_runtime::spawn_blocking(move||Ok(host.lock().map_err(|e|e.to_string())?.status())).await.map_err(|e|e.to_string())?
}
#[tauri::command]
pub async fn dc_module_trust_and_start(app:tauri::AppHandle)->Result<ModuleStatus,String>{
    let host=host(&app);tauri::async_runtime::spawn_blocking(move||{let mut host=host.lock().map_err(|e|e.to_string())?;active(&app)?;host.refresh(true)}).await.map_err(|e|e.to_string())?
}
#[tauri::command]
pub async fn dc_module_set_keep(app:tauri::AppHandle,keep:bool)->Result<ModuleStatus,String>{
    let host=host(&app);tauri::async_runtime::spawn_blocking(move||{let mut host=host.lock().map_err(|e|e.to_string())?;active(&app)?;host.set_keep(keep)}).await.map_err(|e|e.to_string())?
}
#[tauri::command]
pub async fn dc_call(app:tauri::AppHandle,method:String,params:serde_json::Value)->Result<CallResult,String>{
    let mut params=acer_system::device_center_ipc::secret_value(params);
    let host=host(&app);tauri::async_runtime::spawn_blocking(move||{
        let caller={let host=host.lock().map_err(|e|e.to_string())?;
            if active(&app).is_err(){return Ok(CallResult::failure(DcError::new("MODULE_UNTRUSTED","控制台正在退出")))}
            host.caller()
        };
        Ok(match caller {Ok(caller)=>caller.call(&method,params.0.take()),Err(e)=>CallResult::failure(e)})
    }).await.map_err(|e|e.to_string())?
}
pub fn exit(app:&tauri::AppHandle,deadline:std::time::Instant)->Result<(),String>{
    acer_system::device_center_runtime::exit_shared(&host(app),deadline)
}
