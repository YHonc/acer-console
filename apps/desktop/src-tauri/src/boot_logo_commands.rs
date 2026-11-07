use tauri::Manager;
use tauri_plugin_dialog::{DialogExt,FilePath};
use serde_json::{json,Value};

#[tauri::command]
pub async fn get_boot_logo(broker:tauri::State<'_,super::BrokerState>)->Result<Value,String>{
    let broker=broker.0.clone();
    tauri::async_runtime::spawn_blocking(move||{
        let slot=broker.lock().map_err(|e|e.to_string())?;
        slot.as_ref().ok_or("请先授权设备访问")?.get_boot_logo().map_err(|e|e.to_string())
    }).await.map_err(|e|e.to_string())?
}
#[tauri::command]
pub async fn choose_boot_logo(app:tauri::AppHandle,broker:tauri::State<'_,super::BrokerState>)->Result<Option<Value>,String>{
    let broker=broker.0.clone();let gate=app.state::<super::exit_bridge::ExitCoordinator>().gate.clone();
    tauri::async_runtime::spawn_blocking(move||{
        {let slot=broker.lock().map_err(|e|e.to_string())?;if slot.is_none(){return Err("请先授权设备访问".into())}}
        if gate.load(std::sync::atomic::Ordering::SeqCst){return Err("程序正在退出".into())}
        let Some(selected)=app.dialog().file().add_filter("开机图片",&["gif","jpg","jpeg"]).blocking_pick_file()else{return Ok(None)};
        let source=match selected {FilePath::Path(path)=>path,_=>return Err("请选择本机 GIF 或 JPEG 文件".into())};
        let slot=broker.lock().map_err(|e|e.to_string())?;
        if gate.load(std::sync::atomic::Ordering::SeqCst){return Err("程序正在退出".into())}
        let mut plan=slot.as_ref().ok_or("设备连接已断开")?.prepare_boot_logo_image(source.clone()).map_err(|e|e.to_string())?;
        plan["selected_file"]=json!(source.file_name().map(|n|n.to_string_lossy().to_string()));
        Ok(Some(plan))
    }).await.map_err(|e|e.to_string())?
}
enum Action { Reset,Restore(String),Commit(String),Discard(String) }
async fn action(app:tauri::AppHandle,broker:tauri::State<'_,super::BrokerState>,action:Action)->Result<Value,String>{
    let broker=broker.0.clone();let gate=app.state::<super::exit_bridge::ExitCoordinator>().gate.clone();
    tauri::async_runtime::spawn_blocking(move||{
        let slot=broker.lock().map_err(|e|e.to_string())?;
        if gate.load(std::sync::atomic::Ordering::SeqCst){return Err("程序正在退出".into())}
        let client=slot.as_ref().ok_or("请先授权设备访问")?;
        match action {
            Action::Reset=>client.prepare_boot_logo_reset().map_err(|e|e.to_string()),
            Action::Restore(id)=>client.prepare_boot_logo_restore(id).map_err(|e|e.to_string()),
            Action::Discard(id)=>client.discard_boot_logo(id).map_err(|e|e.to_string()),
            Action::Commit(id)=>{
                let report=client.commit_boot_logo(id).map_err(|e|e.to_string())?;
                let(status,message)=if report["status"]=="applied"&&report["success"]==true {
                    ("applied","开机图片文件已应用并读回确认，下次开机查看显示效果")
                }else if report["status"]=="rolled_back" {
                    ("failed","应用失败，四个文件槽已恢复原状态")
                }else {("unconfirmed","结果尚未确认，请保留备份并检查恢复状态")};
                Ok(json!({"status":status,"message":message,"backup_id":report["backup_id"],"report":report}))
            }
        }
    }).await.map_err(|e|e.to_string())?
}
#[tauri::command]
pub async fn prepare_boot_logo_reset(app:tauri::AppHandle,broker:tauri::State<'_,super::BrokerState>)->Result<Value,String>{action(app,broker,Action::Reset).await}
#[tauri::command]
pub async fn prepare_boot_logo_restore(app:tauri::AppHandle,broker:tauri::State<'_,super::BrokerState>,backup_id:String)->Result<Value,String>{action(app,broker,Action::Restore(backup_id)).await}
#[tauri::command]
pub async fn commit_boot_logo(app:tauri::AppHandle,broker:tauri::State<'_,super::BrokerState>,plan_id:String)->Result<Value,String>{action(app,broker,Action::Commit(plan_id)).await}
#[tauri::command]
pub async fn discard_boot_logo(app:tauri::AppHandle,broker:tauri::State<'_,super::BrokerState>,plan_id:String)->Result<Value,String>{action(app,broker,Action::Discard(plan_id)).await}
