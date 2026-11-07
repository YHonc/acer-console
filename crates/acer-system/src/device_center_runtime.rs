//! Owns only the headless child it created. No process-name lookup or hardware IO.
use crate::{device_center_ipc::{self as ipc,CallResult,Client,DcError}, module_discovery::{Discovery,ModuleStatus}};
use serde_json::{json,Value};
use std::{collections::HashMap, path::PathBuf, process::{Child,Command,Stdio}, sync::{Arc,Mutex}, time::{Duration,Instant}};
use tokio::sync::mpsc;
use zeroize::Zeroizing;

pub type EventSink=Arc<dyn Fn(&str,Value)+Send+Sync>;
pub fn lock_until<T>(mutex:&Mutex<T>,deadline:Instant)->Result<std::sync::MutexGuard<'_,T>,String>{
    loop {
        if Instant::now()>=deadline{return Err("退出等待锁超时".into())}
        match mutex.try_lock(){
            Ok(guard)=>return Ok(guard),
            Err(std::sync::TryLockError::Poisoned(e))=>return Err(e.to_string()),
            Err(std::sync::TryLockError::WouldBlock)=>std::thread::sleep(Duration::from_millis(2)),
        }
    }
}
pub fn exit_shared(host:&Mutex<Host>,deadline:Instant)->Result<(),String>{
    lock_until(host,deadline)?.exit_until(deadline)
}
trait Process:Send {fn id(&self)->u32;fn try_wait(&mut self)->std::io::Result<Option<std::process::ExitStatus>>;fn kill(&mut self)->std::io::Result<()>;}
impl Process for Child {
    fn id(&self)->u32{Child::id(self)}
    fn try_wait(&mut self)->std::io::Result<Option<std::process::ExitStatus>>{Child::try_wait(self)}
    fn kill(&mut self)->std::io::Result<()>{Child::kill(self)}
}
#[derive(Default)]
struct RefreshState { active:usize, reads:HashMap<String,Value> }
type Refreshes=Arc<Mutex<HashMap<String,RefreshState>>>;
struct Session { child:Box<dyn Process>, client:Client, events:mpsc::UnboundedReceiver<Value>, pipe:String, nonce:String, refreshes:Refreshes, last_seq:u64 }
struct StartFailure {error:DcError,exit_code:Option<i32>}
impl From<DcError> for StartFailure {
    fn from(error:DcError)->Self{Self{error,exit_code:None}}
}
pub struct Host { pub discovery:Discovery, session:Option<Session>, rt:Arc<tokio::runtime::Runtime>, sink:EventSink, exiting:bool, reconnecting:Option<tokio::task::JoinHandle<Result<(Client,mpsc::UnboundedReceiver<Value>),DcError>>>, reconnect_attempts:Option<mpsc::UnboundedReceiver<u8>> }
#[derive(Clone)]
pub struct Caller { client:Client, rt:Arc<tokio::runtime::Runtime>, refreshes:Refreshes, sink:EventSink }
impl Caller {
    pub fn call(&self,method:&str,params:Value)->CallResult {
        self.rt.block_on(async {
            let mut params=Zeroizing::new(ipc::SecretValue(params));
            let key=params.0["deviceId"].as_str().unwrap_or("lamp").to_owned();
            let read_params=if let Some(id)=params.0.get("deviceId"){json!({"deviceId":id})}else{json!({})};
            self.refreshes.lock().unwrap().entry(key.clone()).or_default().active+=1;
            let result=self.client.call(method,params.0.take(),5000).await;
            let uncertain=result.error.as_ref().is_some_and(|e|e.code=="PARTIAL_WRITE" || (e.code=="TIMEOUT" && is_write(method)));
            {
                let mut states=self.refreshes.lock().unwrap();
                let state=states.get_mut(&key).unwrap();
                if uncertain {
                // Retain the original error and refresh separately. The frontend
                // uses TIMEOUT/PARTIAL_WRITE to display the uncertain-write state.
                let read=match method {"lamp.apply"=>"lamp.read", "lamp.config.save"=>"lamp.config.read", "settings.apply"=>"settings.read", "mouse.dpi.apply"=>"mouse.dpi.read", "mouse.mapping.apply"=>"mouse.mapping.read", "keyboard.lighting.apply"=>"keyboard.lighting.read", _=>"device.snapshots"};
                    state.reads.insert(read.into(),read_params);
                }
                state.active-=1;
            }
            if !result.error.as_ref().is_some_and(|e|e.details["reason"]=="disconnected") {recover(&self.client,&self.refreshes,&key,&self.sink).await;}
            result
        })
    }
}
fn is_write(method:&str)->bool {method.ends_with(".apply") || method=="lamp.config.save"}
async fn recover(client:&Client,refreshes:&Refreshes,key:&str,sink:&EventSink){
    loop {
        let reads={let mut states=refreshes.lock().unwrap();let Some(state)=states.get_mut(key) else{return};
            if state.active!=0 || state.reads.is_empty() || client.is_closed(){return}
            state.active=1;std::mem::take(&mut state.reads)};
        // Retain the existing per-device merge: multiple affected methods use
        // one snapshot request, while a single method keeps its exact read.
        let (method,params)=if reads.len()>1 {("device.snapshots",json!({}))}else{
            let (method,params)=reads.iter().next().unwrap();(method.as_str(),params.clone())
        };
        let result=client.call(method,params,5000).await;
        let retry=!result.ok && (result.error.as_ref().is_some_and(|e|e.details["reason"]=="disconnected") || client.is_closed());
        let mut payload=serde_json::to_value(result).expect("CallResult is serializable");
        payload["deviceId"]=if key=="lamp"{Value::Null}else{json!(key)};
        payload["method"]=json!(method);
        (sink)("dc://readback",payload);
        if retry {
            // Retain the original affected items until an authenticated
            // connection completes their merged read.
            for (method,params) in reads {
                refreshes.lock().unwrap().get_mut(key).unwrap().reads.entry(method).or_insert(params);
            }
        }
        refreshes.lock().unwrap().get_mut(key).unwrap().active-=1;
        if retry{return}
    }
}
impl Host {
    pub fn new(root:PathBuf,config:PathBuf,sink:EventSink)->Result<Self,String>{
        Ok(Self{discovery:Discovery::new(root,config)?,session:None,rt:Arc::new(tokio::runtime::Builder::new_multi_thread().enable_all().build().map_err(|e|e.to_string())?),sink,exiting:false,reconnecting:None,reconnect_attempts:None})
    }
    pub fn status(&self)->ModuleStatus{self.discovery.status.clone()}
    pub fn is_exiting(&self)->bool{self.exiting}
    fn emit(&self){(self.sink)("dc://module",json!(self.status()))}
    pub fn set_keep(&mut self,keep:bool)->Result<ModuleStatus,String>{self.discovery.set_keep(keep)?;self.emit();Ok(self.status())}
    pub fn caller(&self)->Result<Caller,DcError>{
        if self.exiting || self.discovery.status.state!="running" {return Err(DcError::new("MODULE_UNTRUSTED","模块尚未就绪"))}
        self.session.as_ref().map(|s|Caller{client:s.client.clone(),rt:self.rt.clone(),refreshes:s.refreshes.clone(),sink:self.sink.clone()}).ok_or_else(||DcError::new("INTERNAL","模块连接已关闭"))
    }
    pub fn refresh(&mut self,confirm:bool)->Result<ModuleStatus,String>{
        self.refresh_with(confirm,|host,module|host.start(module))
    }
    fn refresh_with(&mut self,confirm:bool,start:impl FnOnce(&Self,crate::device_center_module::ValidatedModule)->Result<Session,StartFailure>)->Result<ModuleStatus,String>{
        if self.exiting{return Err("控制台正在退出".into())}
        if confirm {self.stop(false,"userRestart")?;}
        let previous=self.status();
        let validated=match self.discovery.validate(confirm){Ok(v)=>v,Err(e)=>{self.stop(false,"moduleChanged")?;self.discovery.status=ModuleStatus::new("error",self.discovery.status.keep);self.discovery.status.message=Some(e.clone());self.emit();return Err(e)}};
        match validated {
            None=>self.stop(false,"moduleRemoved")?,
            Some(module)=>{
                if self.session.is_none(){
                    self.discovery.status.state="starting".into();self.emit();
                    match start(self,module){Ok(session)=>{
                        let ack=&session.client.ack;
                        self.discovery.status.state="running".into();self.discovery.status.pid=Some(ack.pid);self.discovery.status.protocol=Some(ack.protocol.clone());
                        self.discovery.status.module_version=Some(ack.module_version.clone());self.discovery.status.capabilities=ack.capabilities.iter().filter(|m|ipc::METHODS.contains(&m.as_str())).cloned().collect();self.session=Some(session);
                    },Err(e)=>{self.start_failed(e);}}
                }else{
                    // Validation must not erase live connection metadata.
                    self.discovery.status=previous;
                }
            }
        }
        self.emit();Ok(self.status())
    }
    fn start(&self,module:crate::device_center_module::ValidatedModule)->Result<Session,StartFailure>{
        let (pipe,nonce)=identity()?;
        let _directory_lock=crate::directory_guard::DirectoryGuard::open(module.entry.parent().unwrap()).map_err(|e|DcError::new("MODULE_UNTRUSTED",e))?;
        let _entry_lock=lock_entry(&module.entry).map_err(|e|DcError::new("MODULE_UNTRUSTED",e))?;
        crate::device_center_module::validate_binding(&module.binding).map_err(|e|DcError::new("MODULE_UNTRUSTED",e))?;
        let mut command=Command::new(&module.entry);
        command.args(["--headless","--pipe",&pipe,"--parent-pid",&std::process::id().to_string(),"--session",&nonce]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        #[cfg(windows)] {use std::os::windows::process::CommandExt;command.creation_flags(0x08000000);}
        let mut child=command.spawn().map_err(|e|DcError::new("INTERNAL",e))?;
        let (tx,events)=mpsc::unbounded_channel();
        let result=self.rt.block_on(connect_pipe(&pipe,&nonce,child.id(),tx));
        match result {Ok(client)=>Ok(Session{child:Box::new(child),client,events,pipe,nonce,refreshes:Arc::default(),last_seq:0}),Err(mut e)=>{
            // No helloAck => never force-terminate an unauthenticated PID.
            // TODO(contract): define authenticated startup cancellation before
            // helloAck; v1 leaves a still-live child to its parent-PID monitor.
            let exit_code=child.try_wait().ok().flatten().and_then(|status|status.code());
            e.message=match exit_code{Some(3)=>"Device Center 已在其他地方运行".into(),Some(code)=>format!("Device Center 退出码 {code}：{}",e.message),None=>e.message};
            Err(StartFailure{error:e,exit_code})
        }}
    }
    fn start_failed(&mut self,failure:StartFailure){
        let error=failure.error;
        self.discovery.status.exit_code=failure.exit_code;
        self.discovery.status.state=if self.discovery.status.exit_code==Some(3){"elsewhere"}else if self.discovery.status.exit_code.is_some(){"error"}else if error.code=="PROTOCOL_MISMATCH"{"incompatible"}else{"error"}.into();
        self.discovery.status.reconnect_attempt=None;
        self.discovery.status.message=Some(error.message);
    }
    fn stop(&mut self,detach:bool,reason:&str)->Result<(),String>{
        self.stop_with_timeout(detach,reason,Duration::from_secs(5))
    }
    fn stop_with_timeout(&mut self,detach:bool,reason:&str,wait:Duration)->Result<(),String>{
        let deadline=Instant::now()+wait;
        self.stop_until(detach,reason,deadline)
    }
    fn stop_until(&mut self,detach:bool,reason:&str,deadline:Instant)->Result<(),String>{
        if let Some(task)=self.reconnecting.take(){task.abort();}
        self.reconnect_attempts=None;
        if let Some(session)=self.session.as_mut(){
            let result=self.rt.block_on(session.client.control_until(if detach{json!({"type":"detach"})}else{json!({"type":"shutdown","reason":reason})},deadline));
            if detach {result.map_err(|e|e.message)?;}
            if !detach {
                loop {
                    if let Some(status)=session.child.try_wait().map_err(|e|e.to_string())?{self.discovery.status.exit_code=status.code();break}
                    if Instant::now()>=deadline {
                        if session.child.id()!=session.client.ack.pid {return Err("子进程 PID 与 helloAck 不一致，无法终止".into())}
                        session.child.kill().map_err(|e|e.to_string())?;
                        break
                    }
                    std::thread::sleep(Duration::from_millis(25).min(deadline.saturating_duration_since(Instant::now())));
                }
            }
        }
        self.session=None;
        Ok(())
    }
    pub fn exit(&mut self)->Result<(),String>{
        self.exit_until(Instant::now()+Duration::from_secs(5))
    }
    pub fn exit_until(&mut self,deadline:Instant)->Result<(),String>{
        if Instant::now()>=deadline{return Err("退出超时".into())}
        self.exiting=true;
        if let Err(e)=self.stop_until(self.discovery.status.keep,"hostExit",deadline){self.exiting=false;self.discovery.status.message=Some(e.clone());self.emit();return Err(e)}
        let exit_code=self.discovery.status.exit_code;
        self.discovery.status=ModuleStatus::new("stopped",self.discovery.status.keep);self.discovery.status.exit_code=exit_code;self.emit();Ok(())
    }
    /// Called by the desktop watcher every 100 ms; never opens any device.
    pub fn tick(&mut self){
        self.tick_with(|pipe,nonce,pid,tx|async move{connect_pipe(&pipe,&nonce,pid,tx).await});
    }
    fn tick_with<F,Fut>(&mut self,connect:F)
    where F:FnMut(String,String,u32,mpsc::UnboundedSender<Value>)->Fut+Send+'static,
          Fut:std::future::Future<Output=Result<Client,DcError>>+Send+'static {
        if self.exiting{return}
        if matches!(self.discovery.status.state.as_str(),"error"|"elsewhere"){return}
        if let Some(session)=self.session.as_mut(){
            if let Ok(Some(status))=session.child.try_wait(){
                self.connection_failed(format!("Device Center 已退出：{status}"),status.code());return
            }
        }
        self.finish_reconnect();
        if matches!(self.discovery.status.state.as_str(),"error"|"elsewhere"){return}
        if self.discovery.poll(Instant::now()){let _=self.refresh(false);}
        let mut disconnected=false;
        if let Some(session)=self.session.as_mut(){
            while let Ok(value)=session.events.try_recv(){
                match value["type"].as_str(){
                    Some("resync")=>{let client=session.client.clone();self.rt.spawn(async move{let _=client.call("device.snapshots",json!({}),5000).await;});},
                    Some("disconnected")=>disconnected=true,
                    Some("event")=>{session.last_seq=value["seq"].as_u64().unwrap_or(session.last_seq);(self.sink)("dc://event",value)},_=>{}
                }
            }
        }
        if disconnected && self.reconnecting.is_none() && self.discovery.status.state!="error"{
            self.begin_reconnect(connect);
        }
    }
    fn begin_reconnect<F,Fut>(&mut self,connect:F)
    where F:FnMut(String,String,u32,mpsc::UnboundedSender<Value>)->Fut+Send+'static,
          Fut:std::future::Future<Output=Result<Client,DcError>>+Send+'static {
        let session=self.session.as_ref().unwrap();
        let (pipe,nonce,pid)=(session.pipe.clone(),session.nonce.clone(),session.child.id());
        let (tx,rx)=mpsc::unbounded_channel();self.reconnect_attempts=Some(rx);
        self.discovery.status.state="reconnecting".into();self.discovery.status.reconnect_attempt=Some(1);
        self.discovery.status.exit_code=None;self.discovery.status.message=None;self.emit();
        self.reconnecting=Some(self.rt.spawn(reconnect_async(pipe,nonce,pid,tx,connect)));
    }
    fn finish_reconnect(&mut self){
        while let Some(attempt)=self.reconnect_attempts.as_mut().and_then(|rx|rx.try_recv().ok()){
            self.discovery.status.reconnect_attempt=Some(attempt);self.emit();
        }
        if !self.reconnecting.as_ref().is_some_and(|t|t.is_finished()){return}
        let task=self.reconnecting.take().unwrap();
        let result=self.rt.block_on(task).unwrap_or_else(|e|Err(DcError::new("INTERNAL",e)));
        // Drain attempts queued between the first drain and task completion.
        while let Some(attempt)=self.reconnect_attempts.as_mut().and_then(|rx|rx.try_recv().ok()){
            self.discovery.status.reconnect_attempt=Some(attempt);self.emit();
        }
        self.reconnect_attempts=None;
        let Some(session)=self.session.as_mut() else{return};
        match result {
            Ok((client,events))=>{
                self.discovery.status.state="running".into();self.discovery.status.reconnect_attempt=None;self.discovery.status.exit_code=None;self.discovery.status.message=None;
                session.client.close();session.client=client;session.events=events;session.last_seq=0;self.discovery.status.capabilities=session.client.ack.capabilities.iter().filter(|m|ipc::METHODS.contains(&m.as_str())).cloned().collect();
                self.discovery.status.protocol=Some(session.client.ack.protocol.clone());self.discovery.status.module_version=Some(session.client.ack.module_version.clone());
                let client=session.client.clone();let refreshes=session.refreshes.clone();let sink=self.sink.clone();
                self.rt.spawn(async move{
                    loop {
                        let keys:Vec<_>=refreshes.lock().unwrap().keys().cloned().collect();
                        for key in keys {recover(&client,&refreshes,&key,&sink).await;}
                        if refreshes.lock().unwrap().values().all(|s|s.active==0 && s.reads.is_empty()) || client.is_closed(){break}
                        tokio::time::sleep(Duration::from_millis(5)).await;
                    }
                });
            },
            Err(e)=>{
                let code=session.child.try_wait().ok().flatten().and_then(|s|s.code());
                self.connection_failed(e.message,code);return
            }
        }
        self.emit();
    }
    fn connection_failed(&mut self,message:String,exit_code:Option<i32>){
        if let Some(task)=self.reconnecting.take(){task.abort();}
        self.reconnect_attempts=None;
        self.discovery.status.state=if exit_code==Some(3){"elsewhere"}else{"error"}.into();
        self.discovery.status.message=Some(if exit_code==Some(3){"Device Center 已在其他地方运行".into()}else{message});
        self.discovery.status.exit_code=exit_code;self.discovery.status.reconnect_attempt=None;
        self.discovery.status.pid=None;self.discovery.status.capabilities.clear();
        if let Some(mut session)=self.session.take(){
                session.client.close();
                // try_wait reaps an exited child. Dropping the owned handle also
                // releases a still-live child without killing an unauthenticated PID.
                let _=session.child.try_wait();
                for key in session.refreshes.lock().unwrap().keys(){
                    session.last_seq+=1;
                    (self.sink)("dc://event",json!({"type":"event","seq":session.last_seq,"name":"device.changed","deviceId":key,"data":{"connected":false}}));
                    session.last_seq+=1;
                    (self.sink)("dc://event",json!({"type":"event","seq":session.last_seq,"name":if key=="lamp"{"lamp.updated"}else{"battery.updated"},"deviceId":if key=="lamp"{Value::Null}else{json!(key)},"data":null}));
                }
        }
        self.emit();
    }
}

async fn reconnect_async<F,Fut>(pipe:String,nonce:String,pid:u32,attempts:mpsc::UnboundedSender<u8>,mut connect:F)->Result<(Client,mpsc::UnboundedReceiver<Value>),DcError>
where F:FnMut(String,String,u32,mpsc::UnboundedSender<Value>)->Fut,
      Fut:std::future::Future<Output=Result<Client,DcError>> {
    let deadline=tokio::time::Instant::now()+Duration::from_secs(5);
    let mut error=DcError::new("TIMEOUT","重连超时");
    for attempt in 0..3 {
        tokio::time::sleep_until((tokio::time::Instant::now()+Duration::from_millis(100*(1<<attempt))).min(deadline)).await;
        if attempt>0 {let _=attempts.send((attempt+1) as u8);}
        let (tx,events)=mpsc::unbounded_channel();
        let budget=deadline.saturating_duration_since(tokio::time::Instant::now())/(3-attempt);
        match tokio::time::timeout(budget,connect(pipe.clone(),nonce.clone(),pid,tx)).await {Ok(Ok(client))=>return Ok((client,events)),Ok(Err(e))=>error=e,Err(_)=>{}}
    }
    Err(error)
}

fn lock_entry(entry:&std::path::Path)->std::io::Result<std::fs::File>{
    let mut options=std::fs::OpenOptions::new();options.read(true);
    #[cfg(windows)] {use std::os::windows::fs::OpenOptionsExt;options.share_mode(1);}
    options.open(entry)
}
#[cfg(windows)]
async fn connect_pipe(pipe:&str,nonce:&str,pid:u32,events:mpsc::UnboundedSender<Value>)->Result<Client,DcError>{
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::Pipes::GetNamedPipeServerProcessId;
    let deadline=Instant::now()+Duration::from_secs(5);
    loop {
        match tokio::net::windows::named_pipe::ClientOptions::new().open(pipe){
            Ok(stream)=>{
                let mut server=0;
                if unsafe{GetNamedPipeServerProcessId(stream.as_raw_handle(),&mut server)}==0 || server!=pid{return Err(DcError::new("UNAUTHORIZED","管道服务端 PID 不一致"))}
                return Client::connect(stream,nonce,pid,events).await
            },
            Err(e) if matches!(e.raw_os_error(),Some(2|231)) && Instant::now()<deadline=>tokio::time::sleep(Duration::from_millis(25)).await,
            Err(e)=>return Err(DcError::new(if Instant::now()>=deadline{"TIMEOUT"}else{"INTERNAL"},e))
        }
    }
}
#[cfg(not(windows))]
async fn connect_pipe(_: &str,_:&str,_:u32,_:mpsc::UnboundedSender<Value>)->Result<Client,DcError>{Err(DcError::new("INTERNAL","Named pipes require Windows"))}

#[cfg(windows)]
fn identity()->Result<(String,String),DcError>{
    use sha2::{Digest,Sha256};
    use windows_sys::Win32::{Foundation::{CloseHandle,LocalFree}, Security::{GetTokenInformation,TokenUser,TOKEN_QUERY,TOKEN_USER,Authorization::ConvertSidToStringSidW,Cryptography::{BCryptGenRandom,BCRYPT_USE_SYSTEM_PREFERRED_RNG}},System::Threading::{GetCurrentProcess,OpenProcessToken}};
    unsafe {
        let mut random=[0u8;16];
        if BCryptGenRandom(std::ptr::null_mut(),random.as_mut_ptr(),16,BCRYPT_USE_SYSTEM_PREFERRED_RNG)<0{return Err(DcError::new("INTERNAL","生成会话 nonce 失败"))}
        let nonce=random.iter().map(|b|format!("{b:02x}")).collect::<String>();
        let mut token=std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(),TOKEN_QUERY,&mut token)==0{return Err(DcError::new("INTERNAL",std::io::Error::last_os_error()))}
        let mut size=0;GetTokenInformation(token,TokenUser,std::ptr::null_mut(),0,&mut size);
        let mut bytes=vec![0usize;(size as usize).div_ceil(std::mem::size_of::<usize>())];
        let ok=GetTokenInformation(token,TokenUser,bytes.as_mut_ptr().cast(),size,&mut size);CloseHandle(token);
        if ok==0{return Err(DcError::new("INTERNAL",std::io::Error::last_os_error()))}
        let user=&*(bytes.as_ptr().cast::<TOKEN_USER>());let mut sid=std::ptr::null_mut();
        if ConvertSidToStringSidW(user.User.Sid,&mut sid)==0{return Err(DcError::new("INTERNAL","无法获取当前用户 SID"))}
        let mut len=0;while *sid.add(len)!=0{len+=1}
        let text=String::from_utf16_lossy(std::slice::from_raw_parts(sid,len));LocalFree(sid.cast());
        let hash=format!("{:x}",Sha256::digest(text.as_bytes()));
        Ok((format!(r"\\.\pipe\AcerConsole.DeviceCenter.{}.{}",&hash[..16],nonce),nonce))
    }
}
#[cfg(not(windows))]
fn identity()->Result<(String,String),DcError>{Err(DcError::new("INTERNAL","Named pipes require Windows"))}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{module_discovery::tests::Fixture,device_center_ipc::{tests::pair,read_frame,write_frame}};
    use std::sync::atomic::{AtomicBool,Ordering};
    struct FakeProcess {pid:u32,exited:Arc<AtomicBool>,killed:Arc<AtomicBool>}
    impl Process for FakeProcess {
        fn id(&self)->u32{self.pid}
        fn try_wait(&mut self)->std::io::Result<Option<std::process::ExitStatus>>{
            #[cfg(windows)] use std::os::windows::process::ExitStatusExt;
            #[cfg(unix)] use std::os::unix::process::ExitStatusExt;
            Ok(self.exited.load(Ordering::SeqCst).then(||std::process::ExitStatus::from_raw(0)))
        }
        fn kill(&mut self)->std::io::Result<()>{self.killed.store(true,Ordering::SeqCst);self.exited.store(true,Ordering::SeqCst);Ok(())}
    }
    fn fake_host(f:&Fixture)->(Host,Arc<AtomicBool>,Arc<AtomicBool>,tokio::io::DuplexStream){
        let mut host=Host::new(f.root.clone(),f.config.clone(),Arc::new(|_,_|{})).unwrap();host.discovery.validate(false).unwrap();
        let (client,server,events)=host.rt.block_on(pair());let exited=Arc::new(AtomicBool::new(false));let killed=Arc::new(AtomicBool::new(false));
        host.discovery.status.state="running".into();host.discovery.status.pid=Some(42);
        host.session=Some(Session{child:Box::new(FakeProcess{pid:42,exited:exited.clone(),killed:killed.clone()}),client,events,pipe:"fake-pipe".into(),nonce:"test-nonce".into(),refreshes:Arc::default(),last_seq:0});
        (host,exited,killed,server)
    }
    #[test] fn directory_disappearance_sends_shutdown_and_rejects_calls(){
        let f=Fixture::new();let (mut host,exited,killed,mut server)=fake_host(&f);
        let server_task=host.rt.spawn(async move{let msg=read_frame(&mut server).await.unwrap();assert_eq!(msg["type"],"shutdown");exited.store(true,Ordering::SeqCst);});
        std::fs::rename(&f.root,f.dir.join("removed")).unwrap();assert_eq!(host.refresh(false).unwrap().state,"absent");assert_eq!(host.caller().err().unwrap().code,"MODULE_UNTRUSTED");assert!(!killed.load(Ordering::SeqCst));host.rt.block_on(server_task).unwrap();
    }
    #[test] fn exit_while_watcher_reconnects_uses_total_deadline(){
        let f=Fixture::new();let (mut host,_,killed,server)=fake_host(&f);drop(server);
        host.rt.block_on(async{for _ in 0..100 {if !host.session.as_ref().unwrap().events.is_empty(){break}tokio::time::sleep(Duration::from_millis(1)).await;}});
        host.tick();assert!(host.reconnecting.is_some());
        let shared=Arc::new(Mutex::new(host));let start=Instant::now();
        assert!(exit_shared(&shared,start+Duration::from_secs(5)).is_ok());
        assert!(start.elapsed()<Duration::from_millis(5250));assert!(killed.load(Ordering::SeqCst));
        let guard=shared.lock().unwrap();let start=Instant::now();assert!(exit_shared(&shared,start+Duration::from_millis(30)).is_err());assert!(start.elapsed()<Duration::from_millis(200));drop(guard);
    }
    #[test] fn disconnected_writes_merge_after_reconnect_and_all_requests_finish(){
        for method in ["settings.apply","lamp.config.save"] {
            let f=Fixture::new();let (mut host,_,_,mut server)=fake_host(&f);
            let mut workers=vec![];
            for _ in 0..2 {let caller=host.caller().unwrap();workers.push(std::thread::spawn(move||caller.call(method,json!({"deviceId":"same","token":"secret"}))));}
            host.rt.block_on(async{read_frame(&mut server).await.unwrap();read_frame(&mut server).await.unwrap();});drop(server);
            for worker in workers {let error=worker.join().unwrap().error.unwrap();assert_eq!(error.code,"TIMEOUT");assert_eq!(error.details["reason"],"disconnected");}
            assert_eq!(host.session.as_ref().unwrap().refreshes.lock().unwrap()["same"].reads.len(),1);
            let (client,mut server,events)=host.rt.block_on(pair());
            // Keep another operation in flight across successful reconnect.
            host.session.as_ref().unwrap().refreshes.lock().unwrap().get_mut("same").unwrap().active=1;
            host.reconnecting=Some(host.rt.spawn(async move{Ok((client,events))}));
            while !host.reconnecting.as_ref().unwrap().is_finished(){std::thread::yield_now();}
            host.finish_reconnect();
            host.rt.block_on(async{assert!(tokio::time::timeout(Duration::from_millis(30),read_frame(&mut server)).await.is_err());});
            host.session.as_ref().unwrap().refreshes.lock().unwrap().get_mut("same").unwrap().active=0;
            host.rt.block_on(async{
                let read=tokio::time::timeout(Duration::from_secs(1),read_frame(&mut server)).await.unwrap().unwrap();assert_eq!(read["method"],if method=="settings.apply"{"settings.read"}else{"lamp.config.read"});
                write_frame(&mut server,&json!({"type":"response","id":read["id"],"ok":true,"result":{}})).await.unwrap();
                assert!(tokio::time::timeout(Duration::from_millis(30),read_frame(&mut server)).await.is_err());
            });
        }
    }
    #[test] fn reconnect_failure_sets_error_and_clears_device_readings(){
        let f=Fixture::new();let (mut host,_,_,_server)=fake_host(&f);
        let emitted=Arc::new(Mutex::new(vec![]));let capture=emitted.clone();host.sink=Arc::new(move|name,value|capture.lock().unwrap().push((name.to_owned(),value)));
        host.session.as_ref().unwrap().refreshes.lock().unwrap().insert("fake".into(),RefreshState::default());
        host.reconnecting=Some(host.rt.spawn(async{Err(DcError::new("TIMEOUT","fake failure"))}));
        while !host.reconnecting.as_ref().unwrap().is_finished(){std::thread::yield_now();}host.finish_reconnect();
        assert_eq!(host.status().state,"error");assert!(host.caller().is_err());
        assert!(emitted.lock().unwrap().iter().any(|(_,v)|v["name"]=="battery.updated" && v["deviceId"]=="fake" && v["data"].is_null()));
    }
    #[test] fn changed_hash_stops_session_as_untrusted(){
        let f=Fixture::new();let (mut host,exited,_,mut server)=fake_host(&f);
        let task=host.rt.spawn(async move{assert_eq!(read_frame(&mut server).await.unwrap()["type"],"shutdown");exited.store(true,Ordering::SeqCst);});
        f.write(2);assert_eq!(host.refresh(false).unwrap().state,"untrusted");assert!(host.caller().is_err());host.rt.block_on(task).unwrap();
    }
    #[test] fn keep_exit_sends_detach_and_no_kill(){
        let f=Fixture::new();let (mut host,_,killed,mut server)=fake_host(&f);host.set_keep(true).unwrap();host.exit().unwrap();
        let msg=host.rt.block_on(read_frame(&mut server)).unwrap();assert_eq!(msg["type"],"detach");assert!(!killed.load(Ordering::SeqCst));assert!(host.caller().is_err());
    }
    #[test] fn failed_detach_preserves_session_and_exit_gate(){
        let f=Fixture::new();let (mut host,_,_,server)=fake_host(&f);host.set_keep(true).unwrap();drop(server);
        assert!(host.exit().is_err());assert!(!host.is_exiting());assert_ne!(host.status().state,"stopped");assert!(host.session.is_some());
    }
    struct Unkillable;
    impl Process for Unkillable {
        fn id(&self)->u32{42}
        fn try_wait(&mut self)->std::io::Result<Option<std::process::ExitStatus>>{Ok(None)}
        fn kill(&mut self)->std::io::Result<()>{Err(std::io::Error::other("fake kill failure"))}
    }
    #[test] fn failed_shutdown_termination_preserves_session(){
        let f=Fixture::new();let (mut host,_,_,_server)=fake_host(&f);host.session.as_mut().unwrap().child=Box::new(Unkillable);
        let start=Instant::now();assert!(host.stop_with_timeout(false,"test",Duration::from_millis(20)).unwrap_err().contains("fake kill failure"));
        assert!(start.elapsed()<Duration::from_millis(500));assert!(host.session.is_some());assert_ne!(host.status().state,"stopped");
    }
    #[test] fn blocked_control_and_shutdown_share_one_budget(){
        for detach in [true,false] {
            let f=Fixture::new();let (mut host,_,killed,_old_server)=fake_host(&f);
            let (client,mut server,events)=host.rt.block_on(ipc::tests::pair_with_capacity(256));
            let sender=client.clone();let blocked=host.rt.spawn(async move{sender.control(json!({"padding":"x".repeat(4096)})).await});
            host.rt.block_on(async{use tokio::io::AsyncReadExt;assert!(server.read_u32_le().await.unwrap()>256);});
            let session=host.session.as_mut().unwrap();session.client=client;session.events=events;
            let start=Instant::now();let result=host.stop_with_timeout(detach,"test",Duration::from_millis(40));
            assert!(start.elapsed()<Duration::from_millis(500));assert_eq!(result.is_err(),detach);assert_eq!(killed.load(Ordering::SeqCst),!detach);
            blocked.abort();
        }
    }
    type Emitted=Arc<Mutex<Vec<(String,Value)>>>;
    fn capture(host:&mut Host)->Emitted {
        let emitted=Arc::new(Mutex::new(vec![]));let copy=emitted.clone();
        host.sink=Arc::new(move|name,value|copy.lock().unwrap().push((name.into(),value)));emitted
    }
    fn module_events(emitted:&Emitted)->Vec<Value>{
        emitted.lock().unwrap().iter().filter(|(name,_)|name=="dc://module").map(|(_,value)|value.clone()).collect()
    }
    fn wait_disconnected(host:&Host){
        host.rt.block_on(async{tokio::time::timeout(Duration::from_secs(1),async{
            while host.session.as_ref().unwrap().events.is_empty(){tokio::time::sleep(Duration::from_millis(1)).await;}
        }).await.unwrap();});
    }
    fn finish_watcher(host:&mut Host){
        let deadline=Instant::now()+Duration::from_secs(2);
        while host.reconnecting.is_some(){assert!(Instant::now()<deadline);host.tick();std::thread::sleep(Duration::from_millis(1));}
    }
    #[test] fn fake_server_disconnect_then_reconnect(){
        let f=Fixture::new();let (mut host,_,_,server)=fake_host(&f);let emitted=capture(&mut host);
        assert_eq!(host.status().state,"running");drop(server);wait_disconnected(&host);
        let (keep_server,mut servers)=mpsc::unbounded_channel();
        host.tick_with(move|pipe,nonce,pid,events|{
            assert_eq!(pipe,"fake-pipe");assert_eq!(nonce,"test-nonce");assert_eq!(pid,42);
            let keep_server=keep_server.clone();
            async move {
                let (a,mut b)=tokio::io::duplex(4096);
                tokio::spawn(async move{let hello=read_frame(&mut b).await.unwrap();assert_eq!(hello["nonce"],"test-nonce");write_frame(&mut b,&ipc::tests::ack()).await.unwrap();keep_server.send(b).unwrap();});
                Client::connect(a,&nonce,pid,events).await
            }
        });
        assert_eq!(host.status().state,"reconnecting");assert_eq!(host.status().reconnect_attempt,Some(1));assert!(host.caller().is_err());
        finish_watcher(&mut host);let _server=servers.try_recv().unwrap();
        assert_eq!(host.status().state,"running");assert!(host.caller().is_ok());
        let events=module_events(&emitted);assert_eq!(events.len(),2);
        assert_eq!(events[0]["state"],"reconnecting");assert_eq!(events[0]["reconnectAttempt"],1);
        assert_eq!(events[1]["state"],"running");assert!(events[1]["reconnectAttempt"].is_null());assert!(events[1]["exitCode"].is_null());
    }
    #[test] fn fake_server_reconnect_exhausted_emits_every_attempt_and_error(){
        let f=Fixture::new();let (mut host,_,_,server)=fake_host(&f);let emitted=capture(&mut host);
        drop(server);wait_disconnected(&host);
        host.tick_with(|_,_,_,_|async{Err(DcError::new("TIMEOUT","fake failure"))});
        finish_watcher(&mut host);assert_eq!(host.status().state,"error");assert!(host.caller().is_err());
        let all_events=module_events(&emitted);let mut events=all_events.clone();events.dedup();assert_eq!(events.len(),4);
        for (index,event) in events[..3].iter().enumerate(){assert_eq!(event["state"],"reconnecting");assert_eq!(event["reconnectAttempt"],index+1);}
        assert_eq!(events[3]["state"],"error");assert!(events[3]["reconnectAttempt"].is_null());assert!(events[3]["exitCode"].is_null());
        host.tick();assert_eq!(module_events(&emitted).len(),all_events.len());
    }
    struct ExitedProcess(i32);
    impl Process for ExitedProcess {
        fn id(&self)->u32{42}
        fn try_wait(&mut self)->std::io::Result<Option<std::process::ExitStatus>>{
            #[cfg(windows)] {use std::os::windows::process::ExitStatusExt;Ok(Some(std::process::ExitStatus::from_raw(self.0 as u32)))}
            #[cfg(unix)] {use std::os::unix::process::ExitStatusExt;Ok(Some(std::process::ExitStatus::from_raw(self.0<<8)))}
        }
        fn kill(&mut self)->std::io::Result<()>{panic!("exited process must not be killed")}
    }
    #[test] fn child_exit_cancels_reconnect_and_emits_exit_code(){
        for code in [0,3,10] {
            let f=Fixture::new();let (mut host,_,_,server)=fake_host(&f);let emitted=capture(&mut host);
            drop(server);wait_disconnected(&host);
            host.tick_with(|_,_,_,_|std::future::pending::<Result<Client,DcError>>());
            host.session.as_mut().unwrap().child=Box::new(ExitedProcess(code));host.tick();
            let expected=if code==3{"elsewhere"}else{"error"};
            assert_eq!(host.status().state,expected);assert_eq!(host.status().exit_code,Some(code));assert!(host.status().reconnect_attempt.is_none());assert!(host.reconnecting.is_none());
            let events=module_events(&emitted);assert_eq!(events.len(),2);assert_eq!(events[1]["state"],expected);assert_eq!(events[1]["exitCode"],code);assert!(events[1]["reconnectAttempt"].is_null());
            host.tick();assert_eq!(module_events(&emitted).len(),2);
            if code==3 {
                // The command's refresh(true) path must still validate the manifest/hash.
                std::fs::write(f.root.join("DeviceCenter.exe"),b"changed").unwrap();
                assert_eq!(host.refresh(true).unwrap().state,"untrusted");assert!(host.session.is_none());
            }
        }
    }
    #[test] fn startup_exit_code_three_is_elsewhere(){
        let f=Fixture::new();let mut host=Host::new(f.root.clone(),f.config.clone(),Arc::new(|_,_|{})).unwrap();let emitted=capture(&mut host);
        let error=DcError::new("TIMEOUT","Device Center 已在其他地方运行");
        host.start_failed(StartFailure{error,exit_code:Some(3)});host.emit();host.tick();
        let events=module_events(&emitted);assert_eq!(events.len(),1);assert_eq!(events[0]["state"],"elsewhere");assert_eq!(events[0]["exitCode"],3);assert!(host.reconnecting.is_none());
    }
    #[cfg(windows)]
    #[test] fn entry_lock_blocks_content_write_and_replacement(){
        let f=Fixture::new();let entry=f.root.join("DeviceCenter.exe");let guard=lock_entry(&entry).unwrap();
        assert!(std::fs::write(&entry,b"changed").is_err());assert!(std::fs::rename(&entry,f.root.join("moved.exe")).is_err());drop(guard);
        assert!(std::fs::write(&entry,b"changed").is_ok());
    }
    #[test] fn partial_write_immediately_reads_back_retains_error(){
        for ok in [true,false] {
        let f=Fixture::new();let (mut host,_,_,mut server)=fake_host(&f);let emitted=capture(&mut host);
        let task=host.rt.spawn(async move{
            let req=read_frame(&mut server).await.unwrap();write_frame(&mut server,&json!({"type":"response","id":req["id"],"ok":false,"error":DcError::new("PARTIAL_WRITE","结果未知，请刷新读回")})).await.unwrap();
            let read=read_frame(&mut server).await.unwrap();assert_eq!(read["method"],"settings.read");assert_eq!(read["params"]["deviceId"],"fake");
            let mut response=if ok{json!({"ok":true,"result":{"readback":true}})}else{json!({"ok":false,"error":DcError::new("DEVICE_DISCONNECTED","read failed")})};
            response["type"]=json!("response");response["id"]=read["id"].clone();
            write_frame(&mut server,&response).await.unwrap();
            // Keep transport alive until the result has been consumed.
            tokio::time::sleep(Duration::from_millis(20)).await;
        });
        let result=host.caller().unwrap().call("settings.apply",json!({"deviceId":"fake","change":{}}));assert!(!result.ok);assert_eq!(result.error.unwrap().code,"PARTIAL_WRITE");host.rt.block_on(task).unwrap();
        let events=emitted.lock().unwrap();let values:Vec<_>=events.iter().filter(|(name,_)|name=="dc://readback").map(|(_,v)|v).collect();
        assert_eq!(values.len(),1);let value=values[0];assert_eq!(value["deviceId"],"fake");assert_eq!(value["method"],"settings.read");assert_eq!(value["ok"],ok);
        if ok{assert_eq!(value["result"]["readback"],true);assert!(value.get("error").is_none());}else{assert_eq!(value["error"]["code"],"DEVICE_DISCONNECTED");assert!(value.get("result").is_none());}
        }
    }
    #[test] fn readback_disconnect_requeues_and_reconnect_delivers_result(){
        let f=Fixture::new();let (mut host,_,_,mut server)=fake_host(&f);let emitted=capture(&mut host);
        let caller=host.caller().unwrap();let worker=std::thread::spawn(move||caller.call("settings.apply",json!({"deviceId":"fake","change":{}})));
        host.rt.block_on(async{
            let req=read_frame(&mut server).await.unwrap();
            write_frame(&mut server,&json!({"type":"response","id":req["id"],"ok":false,"error":DcError::new("PARTIAL_WRITE","unknown")})).await.unwrap();
            assert_eq!(read_frame(&mut server).await.unwrap()["method"],"settings.read");
        });
        drop(server);assert_eq!(worker.join().unwrap().error.unwrap().code,"PARTIAL_WRITE");
        let refreshes=host.session.as_ref().unwrap().refreshes.clone();
        assert_eq!(refreshes.lock().unwrap()["fake"].reads.len(),1);
        let (client,mut server,events)=host.rt.block_on(pair());
        host.reconnecting=Some(host.rt.spawn(async move{Ok((client,events))}));
        while !host.reconnecting.as_ref().unwrap().is_finished(){std::thread::yield_now();}host.finish_reconnect();
        host.rt.block_on(async{
            let read=tokio::time::timeout(Duration::from_secs(1),read_frame(&mut server)).await.unwrap().unwrap();
            assert_eq!(read["method"],"settings.read");assert_eq!(read["params"]["deviceId"],"fake");
            write_frame(&mut server,&json!({"type":"response","id":read["id"],"ok":true,"result":{"fresh":true}})).await.unwrap();
            tokio::time::timeout(Duration::from_secs(1),async{loop{if refreshes.lock().unwrap()["fake"].active==0{break}tokio::time::sleep(Duration::from_millis(1)).await;}}).await.unwrap();
        });
        assert!(refreshes.lock().unwrap()["fake"].reads.is_empty());
        let events=emitted.lock().unwrap();let reads:Vec<_>=events.iter().filter(|(name,_)|name=="dc://readback").map(|(_,v)|v).collect();
        assert_eq!(reads.len(),2);assert_eq!(reads[0]["ok"],false);assert_eq!(reads[0]["error"]["details"]["reason"],"disconnected");assert_eq!(reads[1]["result"]["fresh"],true);
    }
    struct TrackedProcess {inner:FakeProcess,dropped:Arc<AtomicBool>,waited:Arc<AtomicBool>}
    impl Drop for TrackedProcess {fn drop(&mut self){self.dropped.store(true,Ordering::SeqCst);}}
    impl Process for TrackedProcess {
        fn id(&self)->u32{self.inner.id()}
        fn try_wait(&mut self)->std::io::Result<Option<std::process::ExitStatus>>{self.waited.store(true,Ordering::SeqCst);self.inner.try_wait()}
        fn kill(&mut self)->std::io::Result<()>{panic!("terminal cleanup must only release handles")}
    }
    #[test] fn terminal_cleanup_closes_clones_releases_child_and_rebuilds_session(){
        for code in [None,Some(3)] {for confirm in [false,true] {
            let f=Fixture::new();let (mut host,exited,killed,mut server)=fake_host(&f);
            let dropped=Arc::new(AtomicBool::new(false));let waited=Arc::new(AtomicBool::new(false));
            host.session.as_mut().unwrap().child=Box::new(TrackedProcess{inner:FakeProcess{pid:42,exited,killed},dropped:dropped.clone(),waited:waited.clone()});
            let old=host.caller().unwrap();
            host.connection_failed("test".into(),code);
            assert!(host.session.is_none());assert!(dropped.load(Ordering::SeqCst));assert!(waited.load(Ordering::SeqCst));
            host.rt.block_on(async{use tokio::io::AsyncReadExt;let mut byte=[0];assert_eq!(tokio::time::timeout(Duration::from_secs(1),server.read(&mut byte)).await.unwrap().unwrap(),0);});
            assert!(!old.call("device.list",json!({})).ok);
            let (mut replacement,_,_,_server)=fake_host(&f);let session=replacement.session.take().unwrap();
            assert_eq!(host.refresh_with(confirm,|_,_|Ok(session)).unwrap().state,"running");
            assert!(host.session.is_some());assert!(host.caller().is_ok());
        }}
    }
    #[test] fn concurrent_uncertain_writes_merge_readback_after_all_operations(){
        let f=Fixture::new();let (host,_,_,mut server)=fake_host(&f);
        let (done,received)=std::sync::mpsc::channel();
        let mut workers=vec![];
        for _ in 0..2 {
            let caller=host.caller().unwrap();let done=done.clone();
            workers.push(std::thread::spawn(move||{let result=caller.call("settings.apply",json!({"deviceId":"same","change":{}}));done.send(()).unwrap();result}));
        }
        let (first,second)=host.rt.block_on(async{(read_frame(&mut server).await.unwrap(),read_frame(&mut server).await.unwrap())});
        host.rt.block_on(write_frame(&mut server,&json!({"type":"response","id":first["id"],"ok":false,"error":DcError::new("PARTIAL_WRITE","uncertain")}))).unwrap();
        received.recv_timeout(Duration::from_secs(1)).unwrap();
        host.rt.block_on(async{
            assert!(tokio::time::timeout(Duration::from_millis(30),read_frame(&mut server)).await.is_err());
            write_frame(&mut server,&json!({"type":"response","id":second["id"],"ok":false,"error":DcError::new("TIMEOUT","uncertain")})).await.unwrap();
            let refresh=read_frame(&mut server).await.unwrap();assert_eq!(refresh["method"],"settings.read");assert_eq!(refresh["params"]["deviceId"],"same");
            write_frame(&mut server,&json!({"type":"response","id":refresh["id"],"ok":true,"result":{}})).await.unwrap();
        });
        for worker in workers {assert!(!worker.join().unwrap().ok);}
        host.rt.block_on(async{assert!(tokio::time::timeout(Duration::from_millis(30),read_frame(&mut server)).await.is_err());});
    }
    #[test] fn shutdown_timeout_kills_only_owned_matching_pid(){
        for pid in [42,43]{
            let f=Fixture::new();let (mut host,exited,killed,mut server)=fake_host(&f);
            host.session.as_mut().unwrap().child=Box::new(FakeProcess{pid,exited,killed:killed.clone()});
            assert_eq!(host.stop_with_timeout(false,"hostExit",Duration::from_millis(30)).is_ok(),pid==42);
            assert_eq!(killed.load(Ordering::SeqCst),pid==42);
            assert_eq!(host.rt.block_on(read_frame(&mut server)).unwrap()["type"],"shutdown");
        }
    }
    #[test] fn seq_gap_triggers_actual_snapshot_request(){
        let f=Fixture::new();let (mut host,_,_,mut server)=fake_host(&f);
        host.rt.block_on(async{
            write_frame(&mut server,&json!({"type":"event","seq":2,"name":"module.status","data":{"state":"ready"}})).await.unwrap();
            // Wait until the reader has queued a control notification.
            for _ in 0..100 {if !host.session.as_ref().unwrap().events.is_empty(){break}tokio::time::sleep(Duration::from_millis(1)).await;}
        });
        host.tick();
        let request=host.rt.block_on(async{tokio::time::timeout(Duration::from_secs(1),read_frame(&mut server)).await.unwrap().unwrap()});assert_eq!(request["method"],"device.snapshots");
    }
    #[cfg(windows)]
    #[test] fn identity_uses_sid_and_fresh_nonce(){let (a,nonce)=identity().unwrap();let (b,other)=identity().unwrap();assert!(a.starts_with(r"\\.\pipe\AcerConsole.DeviceCenter."));assert_eq!(nonce.len(),32);assert_ne!(nonce,other);assert_ne!(a,b);}
}
