//! Adjacent-folder discovery, complete manifest validation and persisted trust.
use crate::{device_center_module::{self as module, ModuleBinding, ValidatedModule}, device_center_plugin::LOCK};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{path::{Path,PathBuf}, time::{Duration,Instant}};

#[derive(Debug,Clone,Serialize,Deserialize,PartialEq)]
#[serde(rename_all="camelCase")]
pub struct ModuleStatus {
    pub state:String, pub pid:Option<u32>, pub protocol:Option<crate::device_center_ipc::Protocol>,
    pub module_version:Option<String>, pub capabilities:Vec<String>, pub keep:bool, pub message:Option<String>,
    pub reconnect_attempt:Option<u8>, pub exit_code:Option<i32>,
}
impl ModuleStatus {
    pub fn new(state:&str,keep:bool)->Self { Self{state:state.into(),pid:None,protocol:None,module_version:None,capabilities:vec![],keep,message:None,reconnect_attempt:None,exit_code:None} }
}
fn read_config(dir:&Path)->Result<Value,String>{
    match std::fs::read(dir.join("device-center-plugin.json")) {
        Ok(bytes)=> {let value:Value=serde_json::from_slice(&bytes).map_err(|e|e.to_string())?; if !value.is_object(){return Err("模块配置不是对象".into())} Ok(value)},
        Err(e) if e.kind()==std::io::ErrorKind::NotFound=>Ok(json!({})), Err(e)=>Err(e.to_string())
    }
}
fn update_config(dir:&Path,edit:impl FnOnce(&mut Value))->Result<(),String>{
    let _guard=LOCK.lock().map_err(|e|e.to_string())?;
    let mut value=read_config(dir)?; edit(&mut value);
    std::fs::create_dir_all(dir).map_err(|e|e.to_string())?;
    let tmp=dir.join("device-center-plugin.tmp");
    std::fs::write(&tmp,serde_json::to_vec_pretty(&value).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
    std::fs::rename(tmp,dir.join("device-center-plugin.json")).map_err(|e|e.to_string())
}
pub struct Discovery {
    pub root:PathBuf, config:PathBuf, pub status:ModuleStatus,
    observed:Option<String>, changed:Option<Instant>, last_full:Instant,
}
impl Discovery {
    pub fn new(root:PathBuf,config:PathBuf)->Result<Self,String>{
        let status=match read_config(&config){
            Ok(value)=>ModuleStatus::new("absent",value["keep"].as_bool().unwrap_or(false)),
            Err(message)=>{let mut status=ModuleStatus::new("error",false);status.message=Some(message);status}
        };
        Ok(Self{root,config,status,observed:None,changed:None,last_full:Instant::now()})
    }
    pub fn set_keep(&mut self,keep:bool)->Result<(),String>{ update_config(&self.config,|v|v["keep"]=json!(keep))?;self.status.keep=keep;Ok(()) }
    /// Polling avoids an OS notification dependency and also watches parent removal.
    /// Every stable metadata change triggers full manifest and SHA-256 validation.
    pub fn poll(&mut self,now:Instant)->bool {
        if now.saturating_duration_since(self.last_full)>=Duration::from_secs(60){self.last_full=now;return true}
        let signature=signature(&self.root);
        if self.observed.as_ref()!=Some(&signature){self.observed=Some(signature);self.changed=Some(now);return false}
        if self.changed.is_some_and(|t|now.saturating_duration_since(t)>=Duration::from_millis(500)) {self.changed=None;true}else{false}
    }
    pub(crate) fn validate(&mut self,confirm:bool)->Result<Option<ValidatedModule>,String>{
        let keep=self.status.keep;
        if !self.root.exists(){self.status=ModuleStatus::new("absent",keep);return Ok(None)}
        let validated=match module::bind_root(&self.root){Ok(v)=>v,Err(e)=>{self.status=ModuleStatus::new("untrusted",keep);self.status.message=Some(e.to_string());return Ok(None)}};
        let value=read_config(&self.config)?;
        let trusted:Option<ModuleBinding>=value.get("trustSnapshot").cloned().map(serde_json::from_value).transpose().map_err(|e|e.to_string())?;
        let same=trusted.as_ref().is_some_and(|b|b.root==validated.binding.root && b.manifest_sha256==validated.binding.manifest_sha256 && b.files==validated.binding.files);
        if trusted.is_some() && !same && !confirm {
            self.status=ModuleStatus::new("untrusted",keep); self.status.message=Some("模块文件已变更，确认后启动".into());return Ok(None)
        }
        if !same { update_config(&self.config,|v|v["trustSnapshot"]=json!(validated.binding))?; }
        self.status=ModuleStatus::new("stopped",keep);self.status.module_version=Some(validated.binding.version.clone());
        Ok(Some(validated))
    }
}
fn signature(root:&Path)->String {
    fn visit(path:&Path,out:&mut Vec<String>,depth:usize){
        if depth>32{return}
        match std::fs::symlink_metadata(path){
            Ok(m)=>{out.push(format!("{:?}:{}:{:?}",path,m.len(),m.modified()));
                if m.is_dir() && !m.file_type().is_symlink() {
                    #[cfg(windows)] {use std::os::windows::fs::MetadataExt;if m.file_attributes() & 0x400!=0{return}}
                    if let Ok(items)=std::fs::read_dir(path){for p in items.flatten(){visit(&p.path(),out,depth+1)}}
                }
            },Err(e)=>out.push(format!("{:?}:{:?}",path,e.kind()))
        }
    }
    let mut out=vec![];visit(root,&mut out,0);out.sort();out.join("\n")
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use sha2::{Digest,Sha256};
    use std::sync::atomic::{AtomicU64,Ordering};
    static NEXT:AtomicU64=AtomicU64::new(0);
    #[test] fn module_status_serializes_camel_case_fields(){
        let mut status=ModuleStatus::new("reconnecting",false);status.reconnect_attempt=Some(3);
        let value=serde_json::to_value(&status).unwrap();assert_eq!(value["reconnectAttempt"],3);assert!(value["exitCode"].is_null());
        assert!(value.get("reconnect_attempt").is_none());assert!(value.get("exit_code").is_none());
        assert_eq!(serde_json::from_value::<ModuleStatus>(value).unwrap(),status);
        status.state="elsewhere".into();status.reconnect_attempt=None;status.exit_code=Some(3);
        let value=serde_json::to_value(&status).unwrap();assert_eq!(value["exitCode"],3);assert!(value["reconnectAttempt"].is_null());
    }
    pub struct Fixture {pub dir:PathBuf,pub root:PathBuf,pub config:PathBuf}
    impl Fixture {
        pub fn new()->Self{
            let dir=std::env::temp_dir().join(format!("acer-dc-b2-{}-{}",std::process::id(),NEXT.fetch_add(1,Ordering::Relaxed)));
            let root=dir.join("modules/device-center");let config=dir.join("config");std::fs::create_dir_all(&root).unwrap();
            let fixture=Self{dir,root,config};fixture.write(1);fixture
        }
        pub fn write(&self,version:u8){
            let mut bytes=vec![version;128];bytes[..2].copy_from_slice(b"MZ");std::fs::write(self.root.join("DeviceCenter.exe"),&bytes).unwrap();
            std::fs::write(self.root.join("module.json"),serde_json::to_vec(&json!({"schema_version":1,"id":"device-center","name":"Test DC","version":format!("0.1.{version}"),"entry":"DeviceCenter.exe","files":[{"path":"DeviceCenter.exe","sha256":format!("{:x}",Sha256::digest(&bytes))}]})).unwrap()).unwrap();
        }
        pub fn discovery(&self)->Discovery{Discovery::new(self.root.clone(),self.config.clone()).unwrap()}
    }
    impl Drop for Fixture{fn drop(&mut self){let _=std::fs::remove_dir_all(&self.dir);}}
    #[test] fn first_validation_snapshots_and_hash_change_requires_confirmation(){
        let f=Fixture::new();let mut d=f.discovery();assert!(d.validate(false).unwrap().is_some());
        assert!(read_config(&f.config).unwrap()["trustSnapshot"]["files"].is_array());
        f.write(2);assert!(d.validate(false).unwrap().is_none());assert_eq!(d.status.state,"untrusted");
        assert!(d.validate(true).unwrap().is_some());assert_eq!(d.status.module_version.as_deref(),Some("0.1.2"));
    }
    #[test] fn bad_manifest_hash_cannot_be_trusted(){
        let f=Fixture::new();let mut d=f.discovery();d.validate(false).unwrap();std::fs::write(f.root.join("DeviceCenter.exe"),b"changed").unwrap();
        assert!(d.validate(true).unwrap().is_none());assert_eq!(d.status.state,"untrusted");
    }
    #[test] fn periodic_hash_detects_same_size_and_mtime_tampering(){
        use std::io::Write;
        let f=Fixture::new();let mut d=f.discovery();d.validate(false).unwrap();
        let now=Instant::now();d.poll(now);d.poll(now+Duration::from_millis(500));
        let entry=f.root.join("DeviceCenter.exe");let before=signature(&f.root);
        let modified=std::fs::metadata(&entry).unwrap().modified().unwrap();
        let mut file=std::fs::OpenOptions::new().write(true).open(entry).unwrap();file.write_all(&[9;128]).unwrap();
        file.set_times(std::fs::FileTimes::new().set_modified(modified)).unwrap();drop(file);
        assert_eq!(signature(&f.root),before);assert!(!d.poll(now+Duration::from_secs(59)));
        assert!(d.poll(now+Duration::from_secs(60)));assert!(d.validate(false).unwrap().is_none());assert_eq!(d.status.state,"untrusted");
    }
    #[test] fn debounce_500ms_and_absent_directory(){
        let f=Fixture::new();let mut d=f.discovery();let now=Instant::now();assert!(!d.poll(now));assert!(!d.poll(now+Duration::from_millis(499)));assert!(d.poll(now+Duration::from_millis(500)));assert!(!d.poll(now+Duration::from_secs(1)));
        std::fs::rename(&f.root,f.dir.join("removed")).unwrap();assert!(!d.poll(now+Duration::from_secs(2)));assert!(d.poll(now+Duration::from_millis(2500)));assert!(d.validate(false).unwrap().is_none());assert_eq!(d.status.state,"absent");
    }
    #[test] fn legacy_config_fields_preserved_and_keep_reloaded(){
        let f=Fixture::new();std::fs::create_dir_all(&f.config).unwrap();
        std::fs::write(f.config.join("device-center-plugin.json"),br#"{"exe":"old.exe","sha256":"old-hash","custom":{"a":1}}"#).unwrap();
        let mut d=f.discovery();d.set_keep(true).unwrap();d.validate(false).unwrap();let config=read_config(&f.config).unwrap();assert_eq!(config["exe"],"old.exe");assert_eq!(config["sha256"],"old-hash");assert_eq!(config["custom"]["a"],1);assert!(f.discovery().status.keep);
    }
    #[test] fn malformed_config_is_module_error_without_overwriting_it(){
        let f=Fixture::new();std::fs::create_dir_all(&f.config).unwrap();let path=f.config.join("device-center-plugin.json");std::fs::write(&path,b"broken").unwrap();
        let mut d=f.discovery();assert_eq!(d.status.state,"error");assert!(d.validate(false).is_err());assert!(d.set_keep(true).is_err());assert_eq!(std::fs::read(path).unwrap(),b"broken");
    }
}
