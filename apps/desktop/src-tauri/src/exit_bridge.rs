use std::sync::{Arc,Mutex,atomic::{AtomicBool,Ordering}};
use tauri::{Emitter,Manager};
#[derive(Clone,Default)]
pub struct ExitCoordinator {pub state:super::exit_restore::ExitState,pub gate:Arc<AtomicBool>,pub error:Arc<Mutex<Option<String>>>,pub dc_done:Arc<AtomicBool>}
#[tauri::command]
pub fn get_exit_status(app:tauri::AppHandle)->Result<serde_json::Value,String>{
    let state=app.state::<ExitCoordinator>();
    Ok(serde_json::json!({"error":*state.error.lock().map_err(|e|e.to_string())?,"pending":state.state.pending()?}))
}
pub fn attempt(app:&tauri::AppHandle)->bool{
    let coordinator=app.state::<ExitCoordinator>().inner().clone();
    if coordinator.state.pending().is_ok_and(|s|s.allow_exit)&&coordinator.dc_done.load(Ordering::SeqCst){return false}
    if coordinator.gate.swap(true,Ordering::SeqCst){return true}
    let broker=app.state::<super::BrokerState>().0.clone();let app=app.clone();
    tauri::async_runtime::spawn_blocking(move||{
        let result=exit_steps(||->Result<(),String>{
            let slot=broker.lock().map_err(|e|e.to_string())?;
            if app.state::<super::thermal_commands::ThermalState>().owns_fans.load(Ordering::SeqCst){
                super::thermal_commands::stop_locked(&app,slot.as_ref().ok_or("温控恢复需要设备连接")?)?;
            }
            let pending=coordinator.state.pending()?;
            let restore_enabled=super::get_settings(app.clone())?.get("restore_on_exit").and_then(|v|v.as_bool()).unwrap_or(false);
            if !pending.has_receipt||!restore_enabled{coordinator.state.permit_exit_without_restore()?;return Ok(())}
            coordinator.state.set_enabled(true,None)?;
            let client=slot.as_ref().ok_or("恢复需要设备连接，请重新授权后再退出")?;
            match coordinator.state.restore(client)?{
                super::exit_restore::RestoreOutcome::Restored{..}|super::exit_restore::RestoreOutcome::NotNeeded=>Ok(()),
                super::exit_restore::RestoreOutcome::Failed{message,..}|super::exit_restore::RestoreOutcome::Blocked{message}=>Err(message),
                super::exit_restore::RestoreOutcome::InProgress=>Err("退出恢复正在进行".into())
            }
        },|deadline|super::dc_commands::exit(&app,deadline)).map(|()|coordinator.dc_done.store(true,Ordering::SeqCst));
        match result{Ok(())=>app.exit(0),Err(message)=>{*coordinator.error.lock().unwrap()=Some(message.clone());coordinator.gate.store(false,Ordering::SeqCst);super::tray::open(&app);let _=app.emit("exit-restore-error",message);}}
    });true
}
// Complete the original synchronous fan restoration before starting the DC
// budget. There is no outer timeout that can leave a hardware worker running.
fn exit_steps(restore:impl FnOnce()->Result<(),String>,dc:impl FnOnce(std::time::Instant)->Result<(),String>)->Result<(),String>{
    restore()?;
    dc(std::time::Instant::now()+std::time::Duration::from_secs(5))
}
#[tauri::command]
pub async fn exit_without_restore(app:tauri::AppHandle)->Result<(),String>{
    let state=app.state::<ExitCoordinator>().inner().clone();
    if state.gate.swap(true,Ordering::SeqCst){return Err("退出恢复正在进行，请等待结果".into())}
    let broker=app.state::<super::BrokerState>().0.clone();
    tauri::async_runtime::spawn_blocking(move||{
        let result=(||{let _guard=broker.lock().map_err(|e|e.to_string())?;super::dc_commands::exit(&app,std::time::Instant::now()+std::time::Duration::from_secs(5))?;state.dc_done.store(true,Ordering::SeqCst);state.state.permit_exit_without_restore()?;app.exit(0);Ok::<_,String>(())})();
        if result.is_err(){state.gate.store(false,Ordering::SeqCst)}result
    }).await.map_err(|e|e.to_string())?
}

#[cfg(test)] mod tests {
    use super::*;
    #[test] fn dc_timeout_is_reported_only_after_restore_finishes(){
        let finished=AtomicBool::new(false);
        let start=std::time::Instant::now();
        let result=exit_steps(||{std::thread::sleep(std::time::Duration::from_millis(5050));finished.store(true,Ordering::SeqCst);Ok(())},|deadline|{
            assert!(finished.load(Ordering::SeqCst));
            assert!(start.elapsed()>std::time::Duration::from_secs(5));
            assert!(deadline.saturating_duration_since(std::time::Instant::now())>std::time::Duration::from_millis(4950));
            Err("DC timeout".into())
        });
        assert_eq!(result,Err("DC timeout".into()));assert!(finished.load(Ordering::SeqCst));
    }
    #[test] fn failed_restore_does_not_start_dc_exit(){
        assert_eq!(exit_steps(||Err("restore failed".into()),|_|panic!("restoration must finish successfully")),Err("restore failed".into()));
    }
}
