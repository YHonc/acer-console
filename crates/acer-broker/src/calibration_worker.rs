//! Dedicated native lifecycle owner. Hardware objects never cross threads.
//! The directory is supplied by native code, never by an RPC/WebView argument.
use crate::calibration_runtime::NativeSession;
pub use crate::CalibrationAction as Action;
use serde_json::{Value,json};
use std::{path::PathBuf,sync::{Arc,Mutex,mpsc,atomic::{AtomicBool,Ordering}},thread,time::{Duration,Instant}};

struct Request {action:Action,expires:Instant,reply:mpsc::SyncSender<Result<Value,String>>}
trait Active {
    fn status(&self)->Result<Value,String>;
    fn poll(&mut self);
    fn stop(&mut self)->Result<(),String>;
    fn abort(&mut self);
    fn finished(&self)->bool;
}
trait Factory {type Session:Active;fn start(&mut self,id:&str)->Result<Self::Session,String>;fn recover(&mut self,id:&str)->Result<Self::Session,String>;}
impl Active for NativeSession{
    fn status(&self)->Result<Value,String>{serde_json::to_value(self.status()).map_err(|e|e.to_string())}
    fn poll(&mut self){self.poll()}
    fn stop(&mut self)->Result<(),String>{self.stop()}
    fn abort(&mut self){self.abort()}
    fn finished(&self)->bool{let s=self.status();s.session.phase()==acer_protocol::calibration::Phase::Finished&&s.journal_saved}
}
struct NativeFactory{root:PathBuf}
impl Factory for NativeFactory{
    type Session=NativeSession;
    fn start(&mut self,id:&str)->Result<NativeSession,String>{let _root=acer_system::directory_guard::DirectoryGuard::create(&self.root).map_err(|e|e.to_string())?;NativeSession::start(&self.root.join(id))}
    fn recover(&mut self,id:&str)->Result<NativeSession,String>{NativeSession::recover(&self.root.join(id))}
}
fn valid_id(id:&str)->bool{crate::calibration_runtime::valid_session_id(id)}
fn new_id()->Result<String,String>{
    static SEQ:std::sync::atomic::AtomicU64=std::sync::atomic::AtomicU64::new(0);
    let time=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|e|e.to_string())?.as_nanos();
    Ok(format!("{time}-{}-{}",std::process::id(),SEQ.fetch_add(1,Ordering::Relaxed)))
}
/// Query reads hardware only; it never creates a worker, checkpoint or session.
pub(crate) fn query()->Result<Value,acer_protocol::DeviceError>{
    let hardware=acer_device::calibration_hardware::Hardware::connect(acer_device::WritePolicy::ReadOnly)?;
    let observation=hardware.observe();
    let can_start=observation.ac_online==Some(true)&&observation.mode.as_ref().is_some_and(|m|m.supported&&!m.enabled&&m.function_status.iter().all(|v|*v==0));
    Ok(json!({"observation":observation,"start_preconditions_met":can_start,"hardware_validated":false,"validation":"implemented_unverified"}))
}
fn publish(cache:&Mutex<Value>,id:Option<&str>,session:Option<&impl Active>,error:Option<String>){
    let status=session.map(|s|s.status()).transpose();
    let value=match status{Ok(status)=>json!({"session_id":id,"status":status,"error":error}),Err(e)=>json!({"session_id":id,"status":null,"error":e})};
    if let Ok(mut current)=cache.lock(){*current=value}
}
fn run<F:Factory>(mut factory:F,rx:mpsc::Receiver<Request>,stop:Arc<AtomicBool>,cache:Arc<Mutex<Value>>)->Result<(),String>{
    let mut active:Option<F::Session>=None;let mut id:Option<String>=None;let mut last_poll=Instant::now();
    loop{
        if stop.load(Ordering::Acquire){break}
        let request=match rx.recv_timeout(Duration::from_millis(100)){Ok(r)=>Some(r),Err(mpsc::RecvTimeoutError::Timeout)=>None,Err(mpsc::RecvTimeoutError::Disconnected)=>break};
        if let Some(request)=request{
            if stop.load(Ordering::Acquire){let _=request.reply.send(Err("Calibration owner is stopping".into()));break}
            if Instant::now()>request.expires{let _=request.reply.send(Err("Calibration request expired before execution; no action performed".into()));continue}
            let result=(||->Result<(),String>{match request.action{
                Action::Start{}=>{
                    if active.as_ref().is_some_and(|s|!s.finished()){return Err("Earlier calibration session still needs stop or recovery".into())}
                    active=None;id=None;let next=new_id()?;let session=factory.start(&next)?;id=Some(next);active=Some(session);Ok(())
                },
                Action::Recover{session_id}=>{
                    if !valid_id(&session_id){return Err("Invalid native calibration session ID".into())}
                    if active.as_ref().is_some_and(|s|!s.finished()){return Err("Use Abort to recover the current owned session".into())}
                    active=None;id=None;let session=factory.recover(&session_id)?;id=Some(session_id);active=Some(session);Ok(())
                },
                Action::Stop{}=>active.as_mut().ok_or("No owned calibration session")?.stop(),
                Action::Abort{}=>{active.as_mut().ok_or("No owned calibration session")?.abort();Ok(())},
            }})();
            publish(&cache,id.as_deref(),active.as_ref(),result.as_ref().err().cloned());
            let reply=result.and_then(|()|cache.lock().map(|v|v.clone()).map_err(|_|"Calibration status lock poisoned".into()));let _=request.reply.send(reply);
        }
        if last_poll.elapsed()>=Duration::from_secs(1){
            if let Some(session)=&mut active{session.poll();publish(&cache,id.as_deref(),Some(session),None)}last_poll=Instant::now();
        }
    }
    if let Some(session)=&mut active{session.abort();publish(&cache,id.as_deref(),Some(session),None);if !session.finished(){return Err("Calibration cleanup is incomplete; recovery journal retained".into())}}
    Ok(())
}

pub struct Worker {tx:mpsc::SyncSender<Request>,stop:Arc<AtomicBool>,cache:Arc<Mutex<Value>>,done:mpsc::Receiver<Result<(),String>>,handle:Option<thread::JoinHandle<()>>,finished:Option<Result<(),String>>}
#[derive(Clone)]
struct Client {tx:mpsc::SyncSender<Request>,stop:Arc<AtomicBool>}
impl Client{
    fn execute(&self,action:Action)->Result<Value,String>{
        if self.stop.load(Ordering::Acquire){return Err("Calibration owner is stopping".into())}
        let(tx,rx)=mpsc::sync_channel(1);self.tx.try_send(Request{action,expires:Instant::now()+Duration::from_secs(5),reply:tx}).map_err(|e|format!("Calibration request was not queued: {e}"))?;
        rx.recv_timeout(Duration::from_secs(30)).map_err(|e|format!("Calibration result is unconfirmed; inspect status before retrying: {e}"))?
    }
}
/// Lazy helper-owned manager: normal monitoring does not keep an idle thread awake.
#[derive(Default)]
pub(crate) struct Manager{worker:Mutex<Option<Worker>>,closing:AtomicBool}
impl Manager{
    pub(crate) fn query(&self)->Result<Value,acer_protocol::DeviceError>{
        let mut value=query()?;
        let saved=crate::official::config_directory().map_err(|e|e.to_string()).and_then(|root|crate::calibration_runtime::saved_sessions(&root.join("CalibrationSessions")));
        match saved{Ok(saved)=>value["saved_sessions"]=saved,Err(e)=>value["saved_sessions"]=json!({"sessions":[],"error":e})}
        let worker=self.worker.lock().map_err(|_|crate::error("Calibration manager lock poisoned"))?;
        value["lifecycle"]=worker.as_ref().map(|w|w.status()).transpose().map_err(crate::error)?.unwrap_or(Value::Null);Ok(value)
    }
    pub(crate) fn execute(&self,action:Action)->Result<Value,acer_protocol::DeviceError>{
        let client={
            let mut worker=self.worker.lock().map_err(|_|crate::error("Calibration manager lock poisoned"))?;
            if self.closing.load(Ordering::Acquire){return Err(crate::error("Calibration manager is closing"))}
            if worker.is_none(){*worker=Some(Worker::spawn_for_current_user().map_err(crate::error)?)}
            worker.as_ref().unwrap().client()
        };
        client.execute(action).map_err(crate::error)
    }
    pub(crate) fn shutdown(&self)->Result<(),String>{
        self.closing.store(true,Ordering::Release);
        let mut worker=self.worker.lock().map_err(|_|"Calibration manager lock poisoned".to_string())?;
        if let Some(worker)=worker.as_mut(){worker.shutdown(Duration::from_secs(10))?;}Ok(())
    }
}
/// Shutdown is requested even if a timed-out RPC still holds another Arc.
pub(crate) struct ShutdownGuard(pub Arc<Manager>);
impl Drop for ShutdownGuard{fn drop(&mut self){if let Err(e)=self.0.shutdown(){eprintln!("Calibration shutdown unconfirmed: {e}")}}}
impl Worker{
    fn client(&self)->Client{Client{tx:self.tx.clone(),stop:self.stop.clone()}}
    /// Fixed current-user location resolved through the Windows Known Folder API.
    /// The broker already requires the elevated helper to retain the client SID.
    pub fn spawn_for_current_user()->Result<Self,String>{
        let root=crate::official::config_directory().map_err(|e|e.to_string())?.join("CalibrationSessions");Self::spawn(root)
    }
    /// Native supervised integration only until calibration hardware acceptance.
    /// Spawning is idle: no directory creation, hardware writes or recovery runs.
    pub fn spawn(root:PathBuf)->Result<Self,String>{Self::spawn_with(NativeFactory{root})}
    fn spawn_with<F:Factory+Send+'static>(factory:F)->Result<Self,String>{
        let(tx,rx)=mpsc::sync_channel(8);let(done_tx,done)=mpsc::sync_channel(1);let stop=Arc::new(AtomicBool::new(false));let flag=stop.clone();
        let cache=Arc::new(Mutex::new(json!({"session_id":null,"status":null,"error":null})));let shared=cache.clone();
        let handle=thread::Builder::new().name("acer-calibration-owner".into()).spawn(move||{let result=run(factory,rx,flag,shared);let _=done_tx.send(result);}).map_err(|e|e.to_string())?;
        Ok(Self{tx,stop,cache,done,handle:Some(handle),finished:None})
    }
    /// Cached lifecycle state only; null status is not a hardware-mode reading.
    pub fn status(&self)->Result<Value,String>{self.cache.lock().map(|v|v.clone()).map_err(|_|"Calibration status lock poisoned".into())}
    pub fn execute(&self,action:Action)->Result<Value,String>{
        self.client().execute(action)
    }
    pub fn shutdown(&mut self,timeout:Duration)->Result<(),String>{
        self.stop.store(true,Ordering::Release);if let Some(result)=&self.finished{return result.clone()}
        let result=self.done.recv_timeout(timeout).map_err(|e|format!("Calibration owner exit is unconfirmed: {e}"))?;
        if let Some(handle)=self.handle.take(){if handle.join().is_err(){self.finished=Some(Err("Calibration owner panicked".into()));return self.finished.clone().unwrap()}}
        self.finished=Some(result.clone());result
    }
}
impl Drop for Worker{fn drop(&mut self){let _=self.shutdown(Duration::from_secs(5));}}

#[cfg(test)]mod tests{
    use super::*;
    struct FakeFactory{calls:Arc<Mutex<Vec<&'static str>>>}
    struct FakeActive{calls:Arc<Mutex<Vec<&'static str>>>,finished:bool}
    impl Active for FakeActive{
        fn status(&self)->Result<Value,String>{Ok(json!({"finished":self.finished}))}
        fn poll(&mut self){self.calls.lock().unwrap().push("poll")}
        fn stop(&mut self)->Result<(),String>{self.calls.lock().unwrap().push("stop");self.finished=true;Ok(())}
        fn abort(&mut self){self.calls.lock().unwrap().push("abort");self.finished=true}
        fn finished(&self)->bool{self.finished}
    }
    impl Factory for FakeFactory{
        type Session=FakeActive;
        fn start(&mut self,_:&str)->Result<FakeActive,String>{self.calls.lock().unwrap().push("start");Ok(FakeActive{calls:self.calls.clone(),finished:false})}
        fn recover(&mut self,_:&str)->Result<FakeActive,String>{self.calls.lock().unwrap().push("recover");Ok(FakeActive{calls:self.calls.clone(),finished:true})}
    }
    #[test]fn worker_serializes_ownership_and_shutdown_cleans_current_session(){
        let calls=Arc::new(Mutex::new(vec![]));let mut worker=Worker::spawn_with(FakeFactory{calls:calls.clone()}).unwrap();assert!(worker.status().unwrap()["status"].is_null());
        assert!(worker.execute(Action::Start{}).is_ok());assert!(worker.execute(Action::Start{}).is_err());worker.shutdown(Duration::from_secs(2)).unwrap();
        assert_eq!(*calls.lock().unwrap(),vec!["start","abort"]);assert!(worker.execute(Action::Start{}).is_err());
    }
    #[test]fn recovery_only_accepts_native_identifiers(){
        for id in ["", "../1", "C:\\record", "-1", "1-", "1/2"]{assert!(!valid_id(id))}
        let calls=Arc::new(Mutex::new(vec![]));let mut worker=Worker::spawn_with(FakeFactory{calls:calls.clone()}).unwrap();
        assert!(worker.execute(Action::Recover{session_id:"../1".into()}).is_err());assert!(calls.lock().unwrap().is_empty());
        worker.execute(Action::Recover{session_id:"123-456-0".into()}).unwrap();worker.shutdown(Duration::from_secs(2)).unwrap();assert_eq!(calls.lock().unwrap()[0],"recover");
    }
    #[test]fn expired_request_is_rejected_before_start(){
        let calls=Arc::new(Mutex::new(vec![]));let mut worker=Worker::spawn_with(FakeFactory{calls:calls.clone()}).unwrap();let(tx,rx)=mpsc::sync_channel(1);
        worker.tx.try_send(Request{action:Action::Start{},expires:Instant::now()-Duration::from_secs(1),reply:tx}).unwrap();assert!(rx.recv_timeout(Duration::from_secs(2)).unwrap().is_err());
        worker.shutdown(Duration::from_secs(2)).unwrap();assert!(calls.lock().unwrap().is_empty());
    }
}
