use std::sync::{Mutex,atomic::{AtomicBool,Ordering}};
use tauri::{Manager,Emitter};
use acer_device::{ThermalConfig,ThermalStatus};

#[derive(Default)]
pub struct ThermalState { pub owns_fans: AtomicBool, config_lock: Mutex<()> }
fn validate(config:&ThermalConfig)->Result<(),String>{config.cpu.validate().and_then(|_|config.gpu.validate()).map_err(str::to_string)}
fn load_config(dir:&std::path::Path)->Result<Option<ThermalConfig>,String>{
    let path=dir.join("thermal.json");
    let metadata=match std::fs::metadata(&path){Ok(m)=>m,Err(e)if e.kind()==std::io::ErrorKind::NotFound=>return Ok(None),Err(e)=>return Err(e.to_string())};
    if metadata.len()>65536{return Err("曲线配置超过大小限制".into());}
    let bytes=std::fs::read(path).map_err(|e|e.to_string())?;
    let config:ThermalConfig=serde_json::from_slice(&bytes).map_err(|e|e.to_string())?;validate(&config)?;Ok(Some(config))
}
fn store_config(dir:&std::path::Path,config:&ThermalConfig)->Result<(),String>{
    validate(config)?;
    std::fs::create_dir_all(dir).map_err(|e|e.to_string())?;
    std::fs::write(dir.join("thermal.tmp"),serde_json::to_vec_pretty(config).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
    std::fs::rename(dir.join("thermal.tmp"),dir.join("thermal.json")).map_err(|e|e.to_string())
}
fn record(app:&tauri::AppHandle,status:&ThermalStatus) {
    app.state::<ThermalState>().owns_fans.store(status.active||status.restore_pending,Ordering::SeqCst);
    let _=app.emit("thermal-status",status);
}
#[tauri::command]
pub async fn get_thermal_config(app:tauri::AppHandle)->Result<Option<ThermalConfig>,String>{
    tauri::async_runtime::spawn_blocking(move||{
        let state=app.state::<ThermalState>();let _lock=state.config_lock.lock().map_err(|e|e.to_string())?;
        load_config(&app.path().app_config_dir().map_err(|e|e.to_string())?)
    }).await.map_err(|e|e.to_string())?
}
#[tauri::command]
pub async fn save_thermal_config(app:tauri::AppHandle,config:ThermalConfig)->Result<(),String>{
    validate(&config)?;
    tauri::async_runtime::spawn_blocking(move||{
        let state=app.state::<ThermalState>();let _lock=state.config_lock.lock().map_err(|e|e.to_string())?;
        store_config(&app.path().app_config_dir().map_err(|e|e.to_string())?,&config)
    }).await.map_err(|e|e.to_string())?
}
#[tauri::command]
pub async fn get_thermal_status(app:tauri::AppHandle)->Result<ThermalStatus,String>{
    tauri::async_runtime::spawn_blocking(move||{
        let broker=app.state::<super::BrokerState>();let slot=broker.0.lock().map_err(|e|e.to_string())?;
        let status=match slot.as_ref(){Some(c)=>c.thermal_status().map_err(|e|e.to_string())?,None=>{
            if app.state::<ThermalState>().owns_fans.load(Ordering::SeqCst){return Err("温控会话断开，恢复状态未知".into());}
            ThermalStatus::default()
        }};record(&app,&status);Ok(status)
    }).await.map_err(|e|e.to_string())?
}
#[tauri::command]
pub async fn start_thermal(app:tauri::AppHandle,config:ThermalConfig)->Result<ThermalStatus,String>{
    validate(&config)?;
    tauri::async_runtime::spawn_blocking(move||{
        let broker=app.state::<super::BrokerState>();let slot=broker.0.lock().map_err(|e|e.to_string())?;
        if app.state::<super::exit_bridge::ExitCoordinator>().gate.load(Ordering::SeqCst){return Err("控制台正在退出".into());}
        let client=slot.as_ref().ok_or("请先授权设备连接")?;
        // A timeout does not prove that the start request was rejected.
        app.state::<ThermalState>().owns_fans.store(true,Ordering::SeqCst);
        let status=client.start_thermal(config).map_err(|e|e.to_string())?;record(&app,&status);Ok(status)
    }).await.map_err(|e|e.to_string())?
}
pub fn stop_locked(app:&tauri::AppHandle,client:&acer_broker::BrokerClient)->Result<ThermalStatus,String>{
    let status=client.stop_thermal().map_err(|e|e.to_string())?;record(app,&status);
    if status.restore_pending{return Err(format!("自动风扇恢复未确认：{}",status.message));}
    Ok(status)
}
#[tauri::command]
pub async fn stop_thermal(app:tauri::AppHandle)->Result<ThermalStatus,String>{
    tauri::async_runtime::spawn_blocking(move||{
        let broker=app.state::<super::BrokerState>();let slot=broker.0.lock().map_err(|e|e.to_string())?;
        stop_locked(&app,slot.as_ref().ok_or("设备连接不可用，不能确认恢复")?)
    }).await.map_err(|e|e.to_string())?
}
pub fn start_monitor(app:tauri::AppHandle){
    std::thread::spawn(move||{
        let mut last=std::time::Instant::now();
        while !app.state::<super::rules_runtime::RulesState>().stop.load(Ordering::Relaxed){
            std::thread::sleep(std::time::Duration::from_secs(1));
            if last.elapsed()<std::time::Duration::from_secs(15){continue;}last=std::time::Instant::now();
            if !app.state::<ThermalState>().owns_fans.load(Ordering::SeqCst){continue;}
            let broker=app.state::<super::BrokerState>();let Ok(slot)=broker.0.lock()else{continue;};
            if let Some(client)=slot.as_ref(){match client.thermal_status(){Ok(status)=>record(&app,&status),Err(e)=>{let _=app.emit("thermal-error",e.to_string());}}}
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    fn config()->ThermalConfig {
        let curve=serde_json::json!({"points":[{"celsius":40.0,"percent":70},{"celsius":90.0,"percent":100}],"hysteresis_celsius":3.0,"minimum_write_interval_ms":1000,"maximum_sample_age_ms":5000});
        serde_json::from_value(serde_json::json!({"cpu":curve.clone(),"gpu":curve})).unwrap()
    }
    #[test] fn save_reload_replace_and_invalid_save_preserve_original(){
        let dir=std::env::temp_dir().join(format!("acer-thermal-config-test-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        assert!(load_config(&dir).unwrap().is_none());
        let mut value=config();store_config(&dir,&value).unwrap();
        assert_eq!(load_config(&dir).unwrap().unwrap().cpu.points,value.cpu.points);
        value.cpu.hysteresis_celsius=4.0;store_config(&dir,&value).unwrap();
        assert_eq!(load_config(&dir).unwrap().unwrap().cpu.hysteresis_celsius,4.0);
        let original=std::fs::read(dir.join("thermal.json")).unwrap();
        value.cpu.points[1].celsius=20.0;assert!(store_config(&dir,&value).is_err());
        assert_eq!(std::fs::read(dir.join("thermal.json")).unwrap(),original);
        // Delete only the named files created by this test, never a user config tree.
        std::fs::remove_file(dir.join("thermal.json")).unwrap();std::fs::remove_dir(dir).unwrap();
    }
    #[test] fn no_persisted_autostart_or_unvalidated_point_fields(){
        let mut json=serde_json::to_value(config()).unwrap();json["enabled"]=true.into();
        assert!(serde_json::from_value::<ThermalConfig>(json).is_err());
        let mut json=serde_json::to_value(config()).unwrap();json["cpu"]["points"][0]["method"]="SetGamingFanTable".into();
        assert!(serde_json::from_value::<ThermalConfig>(json).is_err());
    }
}
