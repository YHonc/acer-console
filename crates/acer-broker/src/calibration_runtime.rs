//! Native-only calibration lifecycle. No RPC/UI start command is registered.
//! The caller owns one dedicated thread and a new, trusted session directory.
use acer_protocol::calibration::{Acknowledgement, Event, Mode, Phase, Session};
use acer_device::{calibration_events::Listener,calibration_hardware::Hardware,WritePolicy};
use acer_system::{calibration_power::PowerLease,keep_awake::KeepAwake};
use acer_system::directory_guard::{DirectoryGuard,open_record};
use acer_system::power_watch::{PowerWatcher,PowerEvent};
use serde::{Serialize,Deserialize};
use std::{fs,io::{Read,Write},path::{Path,PathBuf},time::Duration};

#[derive(Clone)]
struct State {mode:Option<Mode>,ac:Option<bool>}
enum Signal{Calibration(Event),Resumed}
trait Backend {
    fn observe(&mut self)->State;
    fn subscribe(&mut self)->Result<(),String>;
    fn checkpoint(&mut self)->Result<(),String>;
    fn apply_power(&mut self)->Result<(),String>;
    fn awake(&mut self)->Result<(),String>;
    fn control(&mut self,on:bool)->Result<(Acknowledgement,State),String>;
    fn event(&mut self)->Result<Option<Signal>,String>;
    fn restore_power(&mut self)->Result<(),String>;
    fn release_awake(&mut self)->Result<(),String>;
    fn unsubscribe(&mut self)->Result<(),String>;
    fn persist(&mut self,session:&Session,may_be_active:bool,errors:&[String])->Result<(),String>;
}
struct Runtime<B:Backend>{backend:B,session:Session,may_be_active:bool,errors:Vec<String>,journal_saved:bool}
impl<B:Backend> Runtime<B>{
    fn recover(mut backend:B,may_be_active:bool,errors:Vec<String>)->Self{
        let before=backend.observe();
        let may_be_active=may_be_active&&!before.mode.as_ref().is_some_and(|m|!m.enabled);
        let mut this=Self{backend,session:Session::for_recovery(),may_be_active,errors,journal_saved:false};
        if let Err(e)=this.save(){this.errors.push(e)}
        this.cleanup(true);this
    }
    fn start(mut backend:B)->Result<Self,String>{
        let before=backend.observe();let mode=before.mode.as_ref().ok_or("Calibration state is unknown")?;
        // Pure preflight occurs before subscription, checkpoint and all writes.
        Session::begin(mode,before.ac,true,true).map_err(str::to_owned)?;
        if mode.function_status.iter().any(|v|*v!=0){return Err("Another battery function is active".into())}
        backend.subscribe()?;
        if let Err(e)=backend.checkpoint(){let cleanup=backend.unsubscribe();return Err(format!("Checkpoint failed: {e}; listener cleanup: {cleanup:?}"))}
        let session=Session::begin(mode,before.ac,true,true).map_err(str::to_owned)?;
        let mut this=Self{backend,session,may_be_active:false,errors:vec![],journal_saved:false};
        if let Err(e)=this.save(){this.fail_start(e);return Ok(this)}
        if let Err(e)=this.backend.apply_power(){this.fail_start(e);return Ok(this)}
        if let Err(e)=this.backend.awake(){this.fail_start(e);return Ok(this)}
        // Events already queued before our firmware command cannot belong to it.
        match this.backend.event(){Ok(None)=>(),Ok(Some(_))=>{this.fail_start("Calibration event arrived before start; ownership is uncertain".into());return Ok(this)},Err(e)=>{this.fail_start(e);return Ok(this)}}
        // Persist intent before touching the firmware, including an unknown ACK.
        this.may_be_active=true;
        if let Err(e)=this.save(){this.may_be_active=false;this.fail_start(e);return Ok(this)}
        match this.backend.control(true){
            Ok((ack,after))=>{this.session.start_result(ack,after.mode.as_ref()).map_err(str::to_owned)?;},
            Err(e)=>{this.errors.push(e);this.session.start_result(Acknowledgement::Unknown,None).map_err(str::to_owned)?;},
        }
        if this.session.phase()==Phase::Restoring{this.cleanup(true)}else if let Err(e)=this.save(){this.errors.push(e);this.abort()}
        Ok(this)
    }
    fn save(&mut self)->Result<(),String>{
        self.journal_saved=false;self.backend.persist(&self.session,self.may_be_active,&self.errors)?;self.journal_saved=true;Ok(())
    }
    fn fail_start(&mut self,error:String){
        self.errors.push(error);let _=self.session.start_result(Acknowledgement::Unknown,None);self.cleanup(false);
    }
    fn cleanup(&mut self,stop_hardware:bool){
        // Every cleanup component is attempted even if another one fails.
        if stop_hardware&&self.may_be_active{
            match self.backend.control(false){Ok((ack,after))=>{
                if ack!=Acknowledgement::Accepted{self.errors.push(format!("Stop acknowledgement: {ack:?}"))}
                if after.mode.as_ref().is_some_and(|m|!m.enabled){self.may_be_active=false;}
            },Err(e)=>self.errors.push(e)}
        }
        let power=self.backend.restore_power();let awake=self.backend.release_awake();let listener=self.backend.unsubscribe();
        let after=self.backend.observe();
        if after.mode.as_ref().is_some_and(|m|!m.enabled){self.may_be_active=false;}
        let power_ok=power.is_ok();let awake_ok=awake.is_ok();let listener_ok=listener.is_ok();
        for result in [power,awake,listener]{if let Err(e)=result{self.errors.push(e)}}
        if let Err(e)=self.session.cleanup_result(power_ok,awake_ok,listener_ok,after.mode.as_ref()){self.errors.push(e.into())}
        if let Err(e)=self.save(){self.errors.push(e)}
    }
    fn stop(&mut self)->Result<(),String>{
        self.session.request_stop().map_err(str::to_owned)?;
        // Stop and restoration must still execute when a journal update fails.
        if let Err(e)=self.save(){self.errors.push(e)}
        match self.backend.control(false){Ok((ack,after))=>{
            self.may_be_active=!after.mode.as_ref().is_some_and(|m|!m.enabled);
            self.session.stop_result(ack,after.mode.as_ref()).map_err(str::to_owned)?;
        },Err(e)=>{self.errors.push(e);self.session.stop_result(Acknowledgement::Unknown,None).map_err(str::to_owned)?;}}
        self.cleanup(false);Ok(())
    }
    fn abort(&mut self){
        if self.session.phase()==Phase::Finished {
            if !self.journal_saved {if let Err(e)=self.save(){self.errors.push(e)}}
            return;
        }
        if matches!(self.session.phase(),Phase::Starting|Phase::Running|Phase::Stopping){let _=self.session.abort(None);}
        if self.session.phase()!=Phase::Finished||!self.journal_saved{self.cleanup(true)}
    }
    fn poll(&mut self){
        if self.session.phase()!=Phase::Running{return}
        // Observe provider delivery order, with a bound against event floods.
        for _ in 0..64 {match self.backend.event(){
            Ok(Some(Signal::Calibration(event)))=>{if self.session.observe_event(event){self.cleanup(false);return}},
            Ok(Some(Signal::Resumed))=>{self.errors.push("System resumed during calibration; task aborted for recovery".into());self.abort();return},
            Ok(None)=>break,
            Err(e)=>{self.errors.push(e);self.abort();return},
        }}
        let current=self.backend.observe();
        if current.ac!=Some(true)||!current.mode.as_ref().is_some_and(|m|m.enabled){
            self.errors.push("AC or calibration mode was lost or became unknown".into());self.abort();
        }
    }
}
impl<B:Backend> Drop for Runtime<B>{fn drop(&mut self){self.abort();}}

struct Native {hardware:Hardware,directory:PathBuf,_guard:DirectoryGuard,_lock:fs::File,power:Option<PowerLease>,awake:Option<KeepAwake>,listener:Option<Listener>,power_watcher:Option<PowerWatcher>,last_control:Option<acer_device::calibration_hardware::ControlResult>}
fn state(observation:acer_device::calibration_hardware::Observation)->State{State{mode:observation.mode,ac:observation.ac_online}}
impl Backend for Native{
    fn observe(&mut self)->State{state(self.hardware.observe())}
    fn subscribe(&mut self)->Result<(),String>{
        self.power_watcher=Some(PowerWatcher::start(16).map_err(|e|e.to_string())?);
        self.listener=Some(Listener::start(Duration::from_secs(10))?);Ok(())
    }
    fn checkpoint(&mut self)->Result<(),String>{self.power=Some(PowerLease::prepare(&self.directory.join("power.json")).map_err(|e|e.to_string())?);Ok(())}
    fn apply_power(&mut self)->Result<(),String>{
        let report=self.power.as_mut().ok_or("Missing power checkpoint")?.apply_temporary().map_err(|e|e.to_string())?;
        if report.applied{Ok(())}else{Err(format!("Temporary power settings failed: {report:?}"))}
    }
    fn awake(&mut self)->Result<(),String>{self.awake=Some(KeepAwake::acquire().map_err(|e|e.to_string())?);Ok(())}
    fn control(&mut self,on:bool)->Result<(Acknowledgement,State),String>{
        let result=self.hardware.control(on).map_err(|e|e.to_string())?;
        let output=(result.acknowledgement,State{mode:result.after.mode.clone(),ac:result.after.ac_online});self.last_control=Some(result);Ok(output)
    }
    fn event(&mut self)->Result<Option<Signal>,String>{
        let watcher=self.power_watcher.as_ref().ok_or("No power event watcher")?;
        if !watcher.is_running(){return Err("Power event watcher exited during calibration".into())}
        // Resume has priority over a queued completion event: the interruption
        // invalidates continuity of this owned calibration session.
        for _ in 0..64{match watcher.try_recv(){
            Ok(PowerEvent::Resumed)=>return Ok(Some(Signal::Resumed)),Ok(PowerEvent::PowerChanged)=>(),
            Err(std::sync::mpsc::TryRecvError::Empty)=>break,Err(std::sync::mpsc::TryRecvError::Disconnected)=>return Err("Power event watcher disconnected".into()),
        }}
        match self.listener.as_ref().ok_or("No event listener")?.try_recv(){
            Ok(event)=>Ok(Some(Signal::Calibration(event.event))),Err(std::sync::mpsc::TryRecvError::Empty)=>Ok(None),Err(std::sync::mpsc::TryRecvError::Disconnected)=>Err("Event listener disconnected".into()),
        }
    }
    fn restore_power(&mut self)->Result<(),String>{
        let Some(power)=&mut self.power else{return Ok(())};let report=power.restore();if report.complete{Ok(())}else{Err(format!("Power restoration incomplete: {report:?}"))}
    }
    fn release_awake(&mut self)->Result<(),String>{if let Some(awake)=&mut self.awake{awake.release().map_err(|e|e.to_string())?;}self.awake=None;Ok(())}
    fn unsubscribe(&mut self)->Result<(),String>{
        let mut errors=vec![];
        if let Some(listener)=&mut self.listener{
            let result=listener.stop(Duration::from_secs(5));if listener.exited(){self.listener=None;}if let Err(e)=result{errors.push(e)}
        }
        if let Some(watcher)=&mut self.power_watcher{
            match watcher.shutdown(){Ok(())=>self.power_watcher=None,Err(e)=>errors.push(e.to_string())}
        }
        if errors.is_empty(){Ok(())}else{Err(errors.join("; "))}
    }
    fn persist(&mut self,session:&Session,may_be_active:bool,errors:&[String])->Result<(),String>{
        let bytes=serde_json::to_vec_pretty(&serde_json::json!({"schema":1,"session":session,"hardware_may_be_active":may_be_active,"errors":errors,"last_control":self.last_control})).map_err(|e|e.to_string())?;
        static SEQ:std::sync::atomic::AtomicU64=std::sync::atomic::AtomicU64::new(0);
        let n=SEQ.fetch_add(1,std::sync::atomic::Ordering::Relaxed);
        let stamp=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|e|e.to_string())?.as_nanos();
        let temp=self.directory.join(format!("session-{}-{stamp}-{n}.pending",std::process::id()));
        let mut file=fs::OpenOptions::new().write(true).create_new(true).open(&temp).map_err(|e|e.to_string())?;
        let written=file.write_all(&bytes).and_then(|()|file.sync_all());drop(file);
        let result=written.and_then(|()|fs::rename(&temp,self.directory.join("session.json")));
        if result.is_err(){let _=fs::remove_file(&temp);}result.map_err(|e|e.to_string())
    }
}
#[derive(Serialize)]
pub struct Status<'a>{pub session:&'a Session,pub hardware_may_be_active:bool,pub journal_saved:bool,pub errors:&'a [String]}
pub struct NativeSession{runtime:Runtime<Native>}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredSession{
    schema:u32,session:Session,hardware_may_be_active:bool,errors:Vec<String>,
    #[serde(rename="last_control")] _last_control:Option<serde_json::Value>,
}
fn parse_record(bytes:&[u8])->Result<StoredSession,String>{
    if bytes.len()>131072{return Err("Calibration session record exceeds 128 KiB".into())}
    let record:StoredSession=serde_json::from_slice(bytes).map_err(|e|e.to_string())?;
    if record.schema!=1{return Err("Unsupported calibration session schema".into())}
    if record.session.phase()==Phase::Finished&&record.hardware_may_be_active{return Err("Finished session still claims active hardware".into())}
    Ok(record)
}
fn read_record(path:&Path)->Result<(Vec<u8>,StoredSession),String>{
    let mut bytes=vec![];open_record(path).map_err(|e|e.to_string())?.take(131073).read_to_end(&mut bytes).map_err(|e|e.to_string())?;
    let record=parse_record(&bytes)?;Ok((bytes,record))
}
pub(crate) fn valid_session_id(id:&str)->bool{!id.is_empty()&&id.len()<=64&&id.bytes().all(|b|b.is_ascii_digit()||b==b'-')&&!id.starts_with('-')&&!id.ends_with('-')}
/// Read-only bounded discovery for recovery UI. Never creates missing folders,
/// stops firmware, restores settings, or interprets an old running flag as success.
pub(crate) fn saved_sessions(root:&Path)->Result<serde_json::Value,String>{
    let _root=match DirectoryGuard::open(root){Ok(guard)=>guard,Err(e)if e.kind()==std::io::ErrorKind::NotFound=>return Ok(serde_json::json!({"sessions":[],"truncated":false})),Err(e)=>return Err(e.to_string())};
    let mut sessions=vec![];let mut truncated=false;
    for entry in fs::read_dir(root).map_err(|e|e.to_string())?{
        let entry=entry.map_err(|e|e.to_string())?;let id=entry.file_name().to_string_lossy().into_owned();
        if !valid_session_id(&id)||!entry.file_type().map_err(|e|e.to_string())?.is_dir(){continue}
        if sessions.len()==128{truncated=true;break}
        let path=entry.path();
        let inspect=(||->Result<serde_json::Value,String>{
            let _guard=DirectoryGuard::open(&path).map_err(|e|e.to_string())?;
            let power=PowerLease::load(&path.join("power.json")).map_err(|e|e.to_string())?;
            let record_path=path.join("session.json");
            if !record_path.exists(){
                let recoverable=matches!(power.phase(),acer_system::calibration_power::Phase::Prepared|acer_system::calibration_power::Phase::Cancelled|acer_system::calibration_power::Phase::Conflict);
                return Ok(serde_json::json!({"session_id":id,"phase":"checkpoint_only","power_phase":power.phase(),"recoverable":recoverable,"error":if recoverable{None}else{Some("Missing session intent for an applied power checkpoint")}}))
            }
            let(_,record)=read_record(&record_path)?;
            let power_finished=matches!(power.phase(),acer_system::calibration_power::Phase::Restored|acer_system::calibration_power::Phase::Cancelled);
            let inconsistent=record.session.phase()==Phase::Finished&&!power_finished;
            Ok(serde_json::json!({"session_id":id,"phase":record.session.phase(),"outcome":record.session.outcome(),"power_phase":power.phase(),"hardware_may_be_active":record.hardware_may_be_active,"recoverable":record.session.phase()!=Phase::Finished,"error_count":record.errors.len(),"error":if inconsistent{Some("Session finished but power checkpoint is not restored")}else{None}}))
        })();
        sessions.push(match inspect{Ok(value)=>value,Err(e)=>serde_json::json!({"session_id":id,"phase":"invalid_record","recoverable":false,"error":e.chars().take(500).collect::<String>()})});
    }
    sessions.sort_by(|a,b|a["session_id"].as_str().cmp(&b["session_id"].as_str()));
    Ok(serde_json::json!({"sessions":sessions,"truncated":truncated}))
}
fn owner_lock(directory:&Path)->Result<fs::File,String>{
    use std::os::windows::fs::{OpenOptionsExt,MetadataExt};
    let parent=directory.parent().ok_or("Missing native session root")?;
    let file=fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).share_mode(0).custom_flags(0x0020_0000).open(parent.join("calibration-owner.lock")).map_err(|e|e.to_string())?;
    let meta=file.metadata().map_err(|e|e.to_string())?;
    if !meta.is_file()||meta.file_attributes()&0x400!=0{return Err("Calibration owner lock is not an ordinary file".into())}
    Ok(file)
}
fn ensure_no_pending(root:&Path)->Result<(),String>{
    for entry in fs::read_dir(root).map_err(|e|e.to_string())?{
        let entry=entry.map_err(|e|e.to_string())?;if !entry.file_type().map_err(|e|e.to_string())?.is_dir(){continue}
        let path=entry.path();let _guard=DirectoryGuard::open(&path).map_err(|e|e.to_string())?;let record=path.join("session.json");let power=path.join("power.json");
        if record.exists(){
            let(_,record)=read_record(&record)?;
            if record.session.phase()!=Phase::Finished{return Err(format!("Earlier calibration session needs recovery: {}",path.display()))}
            if !power.exists(){return Err("Finished session is missing its power checkpoint".into())}
            let power=PowerLease::load(&power).map_err(|e|e.to_string())?;
            if !matches!(power.phase(),acer_system::calibration_power::Phase::Restored|acer_system::calibration_power::Phase::Cancelled){return Err("Earlier power checkpoint is not restored".into())}
        }else if power.exists(){return Err(format!("Orphaned power checkpoint needs recovery: {}",path.display()))}
    }
    Ok(())
}
impl NativeSession{
    /// Explicit supervised start only. Directory must be supplied by the native
    /// owner, must not exist, and must not come from a WebView path argument.
    /// This method performs hardware writes; it is not used by read-only probes.
    pub fn start(directory:&Path)->Result<Self,String>{
        let hardware=Hardware::connect(WritePolicy::AllowUnverified).map_err(|e|e.to_string())?;
        let _parent=DirectoryGuard::open(directory.parent().ok_or("Missing session root")?).map_err(|e|e.to_string())?;
        let lock=owner_lock(directory)?;
        ensure_no_pending(directory.parent().ok_or("Missing session root")?)?;
        fs::create_dir(directory).map_err(|e|e.to_string())?;
        let guard=DirectoryGuard::open(directory).map_err(|e|e.to_string())?;
        let backend=Native{hardware,directory:directory.to_path_buf(),_guard:guard,_lock:lock,power:None,awake:None,listener:None,power_watcher:None,last_control:None};
        Ok(Self{runtime:Runtime::start(backend)?})
    }
    /// Explicit recovery of an existing trusted native directory. Loading a
    /// record never re-enters Running and never sends a calibration start.
    pub fn recover(directory:&Path)->Result<Self,String>{
        let guard=DirectoryGuard::open(directory).map_err(|e|e.to_string())?;
        let lock=owner_lock(directory)?;
        let power=PowerLease::load(&directory.join("power.json")).map_err(|e|e.to_string())?;
        let record_path=directory.join("session.json");
        let(may_be_active,errors)=if record_path.exists(){
            let(bytes,record)=read_record(&record_path)?;
            if record.session.phase()==Phase::Finished{return Err("Session already finished; no recovery writes performed".into())}
            // Each attempt preserves its source; an interrupted copy cannot
            // block a later attempt or be mistaken for a complete backup.
            let stamp=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|e|e.to_string())?.as_nanos();
            let source=directory.join(format!("recovery-source-{stamp}.json"));
            let mut file=fs::OpenOptions::new().write(true).create_new(true).open(&source).map_err(|e|e.to_string())?;
            let copied=file.write_all(&bytes).and_then(|()|file.sync_all());drop(file);
            if let Err(e)=copied{let _=fs::remove_file(&source);return Err(e.to_string())}
            (record.hardware_may_be_active,record.errors)
        }else{
            use acer_system::calibration_power::Phase as PowerPhase;
            if !matches!(power.phase(),PowerPhase::Prepared|PowerPhase::Cancelled|PowerPhase::Conflict){return Err("Session intent is missing for an applied checkpoint; cannot attribute hardware ownership".into())}
            (false,vec!["Recovered checkpoint created before session intent".into()])
        };
        let hardware=Hardware::connect(WritePolicy::AllowUnverified).map_err(|e|e.to_string())?;
        let backend=Native{hardware,directory:directory.to_path_buf(),_guard:guard,_lock:lock,power:Some(power),awake:None,listener:None,power_watcher:None,last_control:None};
        Ok(Self{runtime:Runtime::recover(backend,may_be_active,errors)})
    }
    pub fn status(&self)->Status<'_>{Status{session:&self.runtime.session,hardware_may_be_active:self.runtime.may_be_active,journal_saved:self.runtime.journal_saved,errors:&self.runtime.errors}}
    pub fn poll(&mut self){self.runtime.poll()}
    pub fn stop(&mut self)->Result<(),String>{self.runtime.stop()}
    /// Use for AC loss, resume, application exit or explicit recovery retry.
    pub fn abort(&mut self){self.runtime.abort()}
}

#[cfg(test)]mod tests{
    use super::*;use std::{cell::RefCell,rc::Rc,collections::VecDeque};
    struct Fake{calls:Rc<RefCell<Vec<String>>>,enabled:bool,fail:Option<&'static str>,events:VecDeque<Signal>}
    impl Fake{fn op(&mut self,name:&str)->Result<(),String>{self.calls.borrow_mut().push(name.into());if self.fail==Some(name){Err(format!("injected {name}"))}else{Ok(())}}}
    impl Backend for Fake{
        fn observe(&mut self)->State{State{mode:Some(Mode{supported:true,enabled:self.enabled,function_list:3,function_status:[0;5]}),ac:Some(true)}}
        fn subscribe(&mut self)->Result<(),String>{self.op("subscribe")}
        fn checkpoint(&mut self)->Result<(),String>{self.op("checkpoint")}
        fn apply_power(&mut self)->Result<(),String>{self.op("apply_power")}
        fn awake(&mut self)->Result<(),String>{self.op("awake")}
        fn control(&mut self,on:bool)->Result<(Acknowledgement,State),String>{self.op(if on{"start"}else{"stop"})?;self.enabled=on;Ok((Acknowledgement::Accepted,self.observe()))}
        fn event(&mut self)->Result<Option<Signal>,String>{let event=self.events.pop_front();if matches!(event,Some(Signal::Calibration(Event::Completed|Event::Interrupted))){self.enabled=false}Ok(event)}
        fn restore_power(&mut self)->Result<(),String>{self.op("restore")}
        fn release_awake(&mut self)->Result<(),String>{self.op("release")}
        fn unsubscribe(&mut self)->Result<(),String>{self.op("unsubscribe")}
        fn persist(&mut self,_:&Session,active:bool,_:&[String])->Result<(),String>{self.op(if active{"save_active"}else{"save_inactive"})}
    }
    fn fake(fail:Option<&'static str>)->Fake{Fake{calls:Rc::new(RefCell::new(vec![])),enabled:false,fail,events:VecDeque::new()}}
    #[test]fn start_persists_before_firmware_and_stop_restores_all(){
        let backend=fake(None);let calls=backend.calls.clone();let mut run=Runtime::start(backend).unwrap();assert_eq!(run.session.phase(),Phase::Running);
        assert_eq!(&calls.borrow()[..7],&["subscribe","checkpoint","save_inactive","apply_power","awake","save_active","start"]);
        run.stop().unwrap();assert_eq!(run.session.phase(),Phase::Finished);assert!(!run.may_be_active);assert!(run.journal_saved);
        let calls=calls.borrow();assert!(calls.iter().any(|v|v=="restore"));assert!(calls.iter().any(|v|v=="release"));assert!(calls.iter().any(|v|v=="unsubscribe"));
    }
    #[test]fn failed_intent_never_sends_start_and_still_restores(){
        let backend=fake(Some("save_active"));let calls=backend.calls.clone();let run=Runtime::start(backend).unwrap();
        assert!(!calls.borrow().iter().any(|v|v=="start"));assert!(calls.borrow().iter().any(|v|v=="restore"));assert_eq!(run.session.phase(),Phase::Finished);
    }
    #[test]fn stop_failure_does_not_skip_other_cleanup_and_can_retry(){
        let mut run=Runtime::start(fake(None)).unwrap();run.backend.fail=Some("stop");run.stop().unwrap();
        assert_eq!(run.session.phase(),Phase::RecoveryRequired);assert!(run.may_be_active);
        for op in ["restore","release","unsubscribe"]{assert!(run.backend.calls.borrow().iter().any(|v|v==op))}
        run.backend.fail=None;run.abort();assert_eq!(run.session.phase(),Phase::Finished);assert!(!run.may_be_active);
    }
    #[test]fn terminal_event_and_failed_restore_keep_outcome_separate(){
        let mut run=Runtime::start(fake(None)).unwrap();run.backend.events.push_back(Signal::Calibration(Event::Completed));run.backend.fail=Some("restore");run.poll();
        assert_eq!(run.session.phase(),Phase::RecoveryRequired);assert_eq!(run.session.outcome(),Some(acer_protocol::calibration::Outcome::Completed));
        run.backend.fail=None;run.abort();assert_eq!(run.session.phase(),Phase::Finished);
    }
    #[test]fn exit_drop_stops_running_hardware_and_restores(){
        let backend=fake(None);let calls=backend.calls.clone();drop(Runtime::start(backend).unwrap());
        for op in ["stop","restore","release","unsubscribe"]{assert!(calls.borrow().iter().any(|v|v==op))}
    }
    #[test]fn resume_aborts_and_restores_without_claiming_queued_completion(){
        let mut run=Runtime::start(fake(None)).unwrap();
        run.backend.events.push_back(Signal::Resumed);run.backend.events.push_back(Signal::Calibration(Event::Completed));run.poll();
        assert_eq!(run.session.phase(),Phase::Finished);assert_eq!(run.session.outcome(),Some(acer_protocol::calibration::Outcome::Unknown));
        for op in ["stop","restore","release","unsubscribe"]{assert!(run.backend.calls.borrow().iter().any(|v|v==op))}
        assert!(run.errors.iter().any(|e|e.contains("resumed")));
    }
    #[test]fn event_before_start_does_not_get_attributed_to_new_session(){
        let mut backend=fake(None);backend.events.push_back(Signal::Calibration(Event::Completed));let calls=backend.calls.clone();let run=Runtime::start(backend).unwrap();
        assert!(!calls.borrow().iter().any(|v|v=="start"));assert_eq!(run.session.outcome(),Some(acer_protocol::calibration::Outcome::StartFailed));
    }
    #[test]fn restarted_running_session_only_stops_and_restores(){
        let mut backend=fake(None);backend.enabled=true;let calls=backend.calls.clone();let run=Runtime::recover(backend,true,vec![]);
        assert_eq!(run.session.phase(),Phase::Finished);assert_eq!(run.session.outcome(),Some(acer_protocol::calibration::Outcome::Unknown));
        let calls=calls.borrow();assert!(calls.iter().any(|v|v=="stop"));assert!(calls.iter().any(|v|v=="restore"));
        for forbidden in ["start","apply_power","awake","subscribe","checkpoint"]{assert!(!calls.iter().any(|v|v==forbidden))}
    }
    #[test]fn recovery_without_start_intent_never_stops_another_controller(){
        let mut backend=fake(None);backend.enabled=true;let calls=backend.calls.clone();let run=Runtime::recover(backend,false,vec![]);
        assert_eq!(run.session.phase(),Phase::RecoveryRequired);assert!(!calls.borrow().iter().any(|v|v=="stop"));
    }
    #[test]fn recovery_observes_already_off_hardware_without_repeating_stop(){
        let backend=fake(None);let calls=backend.calls.clone();let run=Runtime::recover(backend,true,vec![]);
        assert_eq!(run.session.phase(),Phase::Finished);assert!(!calls.borrow().iter().any(|v|v=="stop"));
    }
    #[test]fn record_parser_rejects_unknown_versions_and_inconsistent_finished_state(){
        let session=Session::for_recovery();let mut value=serde_json::json!({"schema":1,"session":session,"hardware_may_be_active":true,"errors":[],"last_control":null});
        assert!(parse_record(&serde_json::to_vec(&value).unwrap()).is_ok());
        value["schema"]=2.into();assert!(parse_record(&serde_json::to_vec(&value).unwrap()).is_err());value["schema"]=1.into();
        value["session"]["phase"]="finished".into();assert!(parse_record(&serde_json::to_vec(&value).unwrap()).is_err());
        assert!(parse_record(&vec![b' ';131073]).is_err());
    }
    fn test_directory()->PathBuf{
        let stamp=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let root=std::env::temp_dir().join(format!("acer-calibration-runtime-{}-{stamp}",std::process::id()));fs::create_dir(&root).unwrap();root
    }
    #[test]fn sibling_sessions_share_one_exclusive_owner_lock(){
        let root=test_directory();let lock=owner_lock(&root.join("first")).unwrap();
        assert!(owner_lock(&root.join("second")).is_err());drop(lock);drop(owner_lock(&root.join("second")).unwrap());fs::remove_dir_all(root).unwrap();
    }
    #[test]fn new_start_checks_both_session_and_power_completion(){
        let root=test_directory();let dir=root.join("older");fs::create_dir(&dir).unwrap();
        assert!(ensure_no_pending(&root).is_ok());
        let power=serde_json::json!({"schema":1,"phase":"prepared","original":{"scheme":"48c2b03a-8e61-4837-a3ce-621f85306ee3","ac":[2,5,7,43200,0,18000],"dc":[2,5,7,10800,7200,7200]}});
        fs::write(dir.join("power.json"),serde_json::to_vec(&power).unwrap()).unwrap();assert!(ensure_no_pending(&root).is_err());
        let record=serde_json::json!({"schema":1,"session":{"phase":"finished","outcome":"unknown","hardware_stopped":true,"last_event":null},"hardware_may_be_active":false,"errors":[],"last_control":null});
        fs::write(dir.join("session.json"),serde_json::to_vec(&record).unwrap()).unwrap();assert!(ensure_no_pending(&root).is_err());
        let mut power=power;power["phase"]="restored".into();fs::write(dir.join("power.json"),serde_json::to_vec(&power).unwrap()).unwrap();assert!(ensure_no_pending(&root).is_ok());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]fn recovery_discovery_is_read_only_and_reports_invalid_records_individually(){
        let root=test_directory();let missing=root.join("missing");assert_eq!(saved_sessions(&missing).unwrap()["sessions"],serde_json::json!([]));assert!(!missing.exists());
        let valid=root.join("123-1-0");let invalid=root.join("124-1-0");fs::create_dir(&valid).unwrap();fs::create_dir(&invalid).unwrap();
        let bytes=serde_json::to_vec(&serde_json::json!({"schema":1,"phase":"prepared","original":{"scheme":"48c2b03a-8e61-4837-a3ce-621f85306ee3","ac":[2,5,7,43200,0,18000],"dc":[2,5,7,10800,7200,7200]}})).unwrap();
        fs::write(valid.join("power.json"),&bytes).unwrap();fs::write(invalid.join("power.json"),b"broken").unwrap();
        let result=saved_sessions(&root).unwrap();assert_eq!(result["sessions"][0]["phase"],"checkpoint_only");assert_eq!(result["sessions"][0]["recoverable"],true);assert_eq!(result["sessions"][1]["phase"],"invalid_record");
        assert_eq!(fs::read(valid.join("power.json")).unwrap(),bytes);assert!(!valid.join("session.json").exists());assert!(!root.join("calibration-owner.lock").exists());fs::remove_dir_all(root).unwrap();
    }
}
