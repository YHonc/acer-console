use std::sync::{Arc,Mutex,atomic::{AtomicBool,Ordering}};
use std::time::Duration;
use acer_rules::{RuleConfig,RuleEngine,VerifiedTargets,Completion};
use tauri::{Emitter,Manager};
fn reason_text(reason:acer_rules::Reason)->&'static str{
    use acer_rules::Reason::*;
    match reason{
        Disabled=>"规则未启用",UnknownPower=>"电源状态未知，等待重新读取",Baseline=>"已建立当前电源基线",Unchanged=>"等待电源切换",NoTarget=>"当前电源未配置目标模式",CapabilityBlocked=>"设备控制条件暂不满足",TargetBlocked=>"目标模式在当前电源条件下尚未验证",AlreadySelected=>"设备已处于目标模式",Busy=>"上一次操作仍在处理中",Emit=>"正在应用电源规则",WakeRefresh=>"唤醒后重新读取状态",Reconfigured=>"规则已保存，等待电源切换",IdExhausted=>"规则会话需要重新启动"
    }
}
pub struct RulesState {pub engine:Arc<Mutex<RuleEngine>>,pub refresh:Arc<AtomicBool>,pub stop:Arc<AtomicBool>,pub status:Arc<Mutex<String>>}
impl Default for RulesState {fn default()->Self{Self{engine:Arc::new(Mutex::new(RuleEngine::default())),refresh:Arc::new(AtomicBool::new(false)),stop:Arc::new(AtomicBool::new(false)),status:Arc::new(Mutex::new("未启用".into()))}}}
#[tauri::command]
pub fn get_rules(state:tauri::State<RulesState>)->Result<serde_json::Value,String>{
    Ok(serde_json::json!({"config":state.engine.lock().map_err(|e|e.to_string())?.config(),"status":*state.status.lock().map_err(|e|e.to_string())?,"ac_profiles":["quiet","balanced","performance","turbo"],"dc_profiles":[]}))
}
#[tauri::command]
pub fn save_rules(app:tauri::AppHandle,state:tauri::State<RulesState>,config:RuleConfig)->Result<(),String>{
    let dir=app.path().app_config_dir().map_err(|e|e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e|e.to_string())?;
    std::fs::write(dir.join("rules.tmp"),serde_json::to_vec_pretty(&config).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
    std::fs::rename(dir.join("rules.tmp"),dir.join("rules.json")).map_err(|e|e.to_string())?;
    state.engine.lock().map_err(|e|e.to_string())?.reconfigure(config);
    state.refresh.store(true,Ordering::Relaxed);Ok(())
}
pub fn start(app:tauri::AppHandle){
    let state=app.state::<RulesState>();let engine=state.engine.clone();let refresh=state.refresh.clone();let stop=state.stop.clone();let status=state.status.clone();
    if let Ok(dir)=app.path().app_config_dir(){if let Ok(bytes)=std::fs::read(dir.join("rules.json")){match serde_json::from_slice::<RuleConfig>(&bytes){Ok(c)=>{engine.lock().unwrap().reconfigure(c);},Err(e)=>{*status.lock().unwrap()=format!("规则配置读取失败：{e}");}}}}
    let broker=app.state::<super::BrokerState>().0.clone();
    std::thread::spawn(move||{
        use acer_system::power_watch::{PowerWatcher,PowerEvent};
        let watcher=match PowerWatcher::start(16){Ok(w)=>w,Err(e)=>{*status.lock().unwrap()=format!("电源监听失败：{e}");return;}};
        let mut dropped=0;
        let mut heartbeat=std::time::Instant::now();
        while !stop.load(Ordering::Relaxed){
            let event=match watcher.recv_timeout(Duration::from_secs(1)) {
                Ok(event)=>Some(event),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout)=>None,
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected)=>{
                    engine.lock().unwrap().on_wake();
                    *status.lock().unwrap()="电源监听已停止，请重启控制台以恢复自动规则".into();
                    let _=app.emit("device-refresh",());
                    break;
                }
            };
            let mut resumed=matches!(event,Some(PowerEvent::Resumed));
            let mut notified=event.is_some();
            while let Ok(next)=watcher.try_recv(){notified=true;resumed|=matches!(next,PowerEvent::Resumed);}
            let refresh_now=refresh.swap(false,Ordering::Relaxed);
            if heartbeat.elapsed()>=Duration::from_secs(25){
                heartbeat=std::time::Instant::now();
                let restore_pending=app.state::<super::exit_bridge::ExitCoordinator>().state.pending().is_ok_and(|p|p.has_receipt)&&super::get_settings(app.clone()).ok().and_then(|s|s["restore_on_exit"].as_bool()).unwrap_or(false);
                let tray_active=app.get_webview_window("main").is_none();
                if engine.lock().unwrap().config().enabled||restore_pending||tray_active{
                    if let Some(client)=broker.lock().unwrap().as_ref(){if let Err(e)=client.keep_alive(){engine.lock().unwrap().on_wake();*status.lock().unwrap()=format!("读取组件已断开，请重新授权：{e}");}}
                }
            }
            if !notified&&!refresh_now{continue}
            let new_dropped=watcher.dropped_events();let overflow=new_dropped!=dropped;dropped=new_dropped;
            if resumed||overflow{engine.lock().unwrap().on_wake();}
            if (resumed||overflow)&&app.state::<super::thermal_commands::ThermalState>().owns_fans.load(Ordering::SeqCst){
                if let Some(client)=broker.lock().unwrap().as_ref(){if let Err(e)=super::thermal_commands::stop_locked(&app,client){let _=app.emit("thermal-error",e);}}
            }
            let _=app.emit("device-refresh",());
            if !engine.lock().unwrap().config().enabled{continue}
            let slot=broker.lock().unwrap();let Some(client)=slot.as_ref()else{engine.lock().unwrap().on_wake();*status.lock().unwrap()="等待设备授权".into();continue};
            let (snapshot,caps)=match (client.snapshot(),client.capabilities()){(Ok(s),Ok(c))=>(s,c),_=>{engine.lock().unwrap().on_wake();*status.lock().unwrap()="设备读取失败，请重新授权".into();continue}};
            let targets=VerifiedTargets{ac:vec![acer_protocol::Profile::Quiet,acer_protocol::Profile::Balanced,acer_protocol::Profile::Performance,acer_protocol::Profile::Turbo],dc:vec![]};
            let decision=engine.lock().unwrap().observe_snapshot(&snapshot,&caps,&targets);
            *status.lock().unwrap()=reason_text(decision.reason).into();
            if let Some(action)=decision.action{
                if !engine.lock().unwrap().is_current(action.id){continue}
                if app.state::<super::exit_bridge::ExitCoordinator>().gate.load(Ordering::SeqCst){engine.lock().unwrap().finish(action.id,Completion::Unconfirmed);continue}
                let result=client.apply(action.command);
                let completion=match &result{Ok(r) if r.status==acer_protocol::ApplyStatus::Applied=>Completion::Applied,Ok(r) if r.status==acer_protocol::ApplyStatus::Unconfirmed=>Completion::Unconfirmed,_=>Completion::Failed};
                engine.lock().unwrap().finish(action.id,completion);
                *status.lock().unwrap()=match result{Ok(r)=>match r.status{acer_protocol::ApplyStatus::Applied=>"电源规则已应用并读回确认".into(),acer_protocol::ApplyStatus::Unconfirmed=>"规则请求已发送，但读回未确认".into(),_=>format!("规则未应用：{}",r.message)},Err(e)=>e.to_string()};
                let _=app.emit("device-refresh",());
            }
        }
    });
}
