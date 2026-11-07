//! The six fixed power settings used by local Acer battery calibration.
//! Capture is read-only. Applying temporary settings requires a durable lease;
//! callers must explicitly restore it and retain failed recovery journals.
use crate::{error,SystemError};
use serde::{Deserialize,Serialize};
use std::{fs,io::Write,path::{Path,PathBuf}};

#[repr(C)]
#[derive(Debug,Clone,Copy,PartialEq,Eq,Serialize,Deserialize)]
#[serde(try_from="String",into="String")]
pub struct SchemeId { a:u32,b:u16,c:u16,d:[u8;8] }
impl TryFrom<String> for SchemeId {
    type Error=SystemError;
    fn try_from(value:String)->Result<Self,Self::Error>{
        if value.len()!=36||!value.bytes().enumerate().all(|(i,b)|if [8,13,18,23].contains(&i){b==b'-'}else{b.is_ascii_hexdigit()}){return Err(error("Invalid power scheme GUID"))}
        let compact=value.replace('-',"");let mut bytes=[0u8;16];
        for(i,byte)in bytes.iter_mut().enumerate(){*byte=u8::from_str_radix(&compact[i*2..i*2+2],16).map_err(error)?;}
        if bytes==[0;16]{return Err(error("Empty power scheme GUID"))}
        Ok(Self{a:u32::from_be_bytes(bytes[0..4].try_into().unwrap()),b:u16::from_be_bytes(bytes[4..6].try_into().unwrap()),c:u16::from_be_bytes(bytes[6..8].try_into().unwrap()),d:bytes[8..16].try_into().unwrap()})
    }
}
impl From<SchemeId> for String {fn from(g:SchemeId)->Self{format!("{:08x}-{:04x}-{:04x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",g.a,g.b,g.c,g.d[0],g.d[1],g.d[2],g.d[3],g.d[4],g.d[5],g.d[6],g.d[7])}}
#[derive(Debug,Clone,Copy,Serialize)]
#[serde(rename_all="snake_case")]
pub enum Setting {CriticalAction,CriticalLevel,ReserveLevel,HibernateAfter,SleepAfter,DisplayAfter}
pub const SETTINGS:[Setting;6]=[Setting::CriticalAction,Setting::CriticalLevel,Setting::ReserveLevel,Setting::HibernateAfter,Setting::SleepAfter,Setting::DisplayAfter];
const TEMPORARY:[u32;6]=[0,1,1,0,0,0];
fn ids(setting:Setting)->(SchemeId,SchemeId){
    let(group,value)=match setting {
        Setting::CriticalAction=>("e73a048d-bf27-4f12-9731-8b2076e8891f","637ea02f-bbcb-4015-8e2c-a1c7b9c0b546"),
        Setting::CriticalLevel=>("e73a048d-bf27-4f12-9731-8b2076e8891f","9a66d8d7-4ff7-4ef9-b5a2-5a326ca2a469"),
        Setting::ReserveLevel=>("e73a048d-bf27-4f12-9731-8b2076e8891f","f3c5027d-cd16-4930-aa6b-90db844a8f00"),
        Setting::HibernateAfter=>("238c9fa8-0aad-41ed-83f4-97be242c8f20","9d7815a6-7ee4-497e-8888-515a05f02364"),
        Setting::SleepAfter=>("238c9fa8-0aad-41ed-83f4-97be242c8f20","29f6c1db-86da-48c5-9fdb-f2b67b1f44da"),
        Setting::DisplayAfter=>("7516b95f-f776-4464-8c53-06167f40cc99","3c0bc021-c8a8-4e07-a973-6b14cbcb2b7e"),
    };(group.to_string().try_into().unwrap(),value.to_string().try_into().unwrap())
}
#[derive(Debug,Clone,PartialEq,Eq,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PowerSnapshot {pub scheme:SchemeId,pub ac:[u32;6],pub dc:[u32;6]}
#[derive(Debug,Clone,Copy,PartialEq,Eq,Serialize,Deserialize)]
#[serde(rename_all="snake_case")]
pub enum Phase {Prepared,Applying,Applied,Restoring,Restored,RecoveryRequired,Conflict,Cancelled}
#[derive(Debug,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {schema:u32,phase:Phase,original:PowerSnapshot}
#[derive(Debug,Serialize)]
pub struct RestoreReport {pub attempted:bool,pub readback_matches:Option<bool>,pub active_scheme_preserved:bool,pub errors:Vec<String>,pub complete:bool}
#[derive(Debug,Serialize)]
pub struct ApplyReport {pub applied:bool,pub errors:Vec<String>,pub rollback:Option<RestoreReport>}
trait Backend {
    fn active(&mut self)->Result<SchemeId,SystemError>;
    fn read(&mut self,scheme:SchemeId)->Result<PowerSnapshot,SystemError>;
    fn write(&mut self,scheme:SchemeId,dc:bool,index:usize,value:u32)->Result<(),SystemError>;
    fn activate(&mut self,scheme:SchemeId)->Result<(),SystemError>;
}
fn save(path:&Path,journal:&Journal,create:bool)->Result<(),SystemError>{
    let bytes=serde_json::to_vec_pretty(journal).map_err(error)?;
    // A leftover temporary file from an interrupted save must not block recovery.
    static SEQUENCE:std::sync::atomic::AtomicU64=std::sync::atomic::AtomicU64::new(0);
    let target=if create{path.to_path_buf()}else{
        let mut name=path.as_os_str().to_os_string();
        let stamp=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(error)?.as_nanos();
        name.push(format!(".{}-{stamp}-{}.pending",std::process::id(),SEQUENCE.fetch_add(1,std::sync::atomic::Ordering::Relaxed)));
        PathBuf::from(name)
    };
    let mut file=fs::OpenOptions::new().write(true).create_new(true).open(&target).map_err(error)?;
    let written=file.write_all(&bytes).and_then(|()|file.sync_all());drop(file);
    let result=written.and_then(|()|if create{Ok(())}else{fs::rename(&target,path)});
    if result.is_err()&&!create{let _=fs::remove_file(&target);}
    result.map_err(error)
}
struct Lease<B>{backend:B,path:PathBuf,journal:Journal}
impl<B:Backend> Lease<B>{
    fn prepare(mut backend:B,path:PathBuf)->Result<Self,SystemError>{
        let scheme=backend.active()?;let original=backend.read(scheme)?;
        let journal=Journal{schema:1,phase:Phase::Prepared,original};
        save(&path,&journal,true)?;Ok(Self{backend,path,journal})
    }
    fn phase(&mut self,phase:Phase)->Result<(),SystemError>{
        let previous=self.journal.phase;self.journal.phase=phase;
        if let Err(e)=save(&self.path,&self.journal,false){self.journal.phase=previous;return Err(e)}Ok(())
    }
    fn apply(&mut self)->Result<ApplyReport,SystemError>{
        if self.journal.phase!=Phase::Prepared{return Err(error("Power lease is not in Prepared state"))}
        let original=self.journal.original.clone();
        if self.backend.active()?!=original.scheme||self.backend.read(original.scheme)?!=original{
            self.phase(Phase::Conflict)?;return Err(error("Power scheme changed since capture; no settings written"))
        }
        self.phase(Phase::Applying)?;
        let mut errors=vec![];
        'settings: for dc in [false,true]{for(index,value)in TEMPORARY.into_iter().enumerate(){
            if let Err(e)=self.backend.write(original.scheme,dc,index,value){errors.push(e.message);break 'settings}
        }}
        if errors.is_empty(){match self.backend.active(){
            Ok(active) if active==original.scheme=>if let Err(e)=self.backend.activate(original.scheme){errors.push(e.message)},
            Ok(_)=>errors.push("Active scheme changed during preparation".into()),Err(e)=>errors.push(e.message),
        }}
        if errors.is_empty(){match self.backend.read(original.scheme){
            Ok(state) if state.ac==TEMPORARY&&state.dc==TEMPORARY=>(),
            Ok(_)=>errors.push("Temporary power settings readback differs".into()),Err(e)=>errors.push(e.message),
        }}
        if errors.is_empty(){
            if let Err(e)=self.phase(Phase::Applied){errors.push(e.message)}else{return Ok(ApplyReport{applied:true,errors,rollback:None})}
        }
        let rollback=self.restore();Ok(ApplyReport{applied:false,errors,rollback:Some(rollback)})
    }
    fn restore(&mut self)->RestoreReport{
        let mut errors=vec![];
        if matches!(self.journal.phase,Phase::Prepared|Phase::Conflict|Phase::Restored|Phase::Cancelled){
            if matches!(self.journal.phase,Phase::Prepared|Phase::Conflict){if let Err(e)=self.phase(Phase::Cancelled){errors.push(e.message)}}
            return RestoreReport{attempted:false,readback_matches:None,active_scheme_preserved:true,complete:errors.is_empty(),errors};
        }
        // A journal update failure must never prevent an already-needed restoration.
        if let Err(e)=self.phase(Phase::Restoring){errors.push(e.message)}
        let original=self.journal.original.clone();
        for dc in [false,true]{let values=if dc{original.dc}else{original.ac};for(index,value)in values.into_iter().enumerate(){
            if let Err(e)=self.backend.write(original.scheme,dc,index,value){errors.push(format!("{} index {index}: {}",if dc{"DC"}else{"AC"},e.message))}
        }}
        let active_scheme_preserved=match self.backend.active(){
            Ok(active) if active==original.scheme=>match self.backend.activate(original.scheme){Ok(())=>true,Err(e)=>{errors.push(e.message);false}},
            Ok(_)=>true, // Restore stored values without switching back from a user-selected plan.
            Err(e)=>{errors.push(e.message);false},
        };
        let readback_matches=match self.backend.read(original.scheme){Ok(state)=>state==original,Err(e)=>{errors.push(e.message);false}};
        let complete=readback_matches&&active_scheme_preserved&&errors.is_empty();
        if let Err(e)=self.phase(if complete{Phase::Restored}else{Phase::RecoveryRequired}){errors.push(e.message)}
        RestoreReport{attempted:true,readback_matches:Some(readback_matches),active_scheme_preserved,complete:complete&&errors.is_empty(),errors}
    }
}

/// Capture the current scheme and both independent six-value arrays, without writes.
pub fn capture()->Result<PowerSnapshot,SystemError>{let mut backend=Native;let scheme=backend.active()?;backend.read(scheme)}
pub struct PowerLease {lease:Lease<Native>}
impl PowerLease {
    /// The native calibration owner supplies a new journal filename in its data directory.
    /// Does not create directories, change settings, keep the system awake, or start calibration.
    pub fn prepare(journal:&Path)->Result<Self,SystemError>{Ok(Self{lease:Lease::prepare(Native,journal.to_path_buf())?})}
    pub fn original(&self)->&PowerSnapshot{&self.lease.journal.original}
    pub fn apply_temporary(&mut self)->Result<ApplyReport,SystemError>{self.lease.apply()}
    pub fn restore(&mut self)->RestoreReport{self.lease.restore()}
    /// Explicit crash recovery only; loading the journal alone makes no changes.
    pub fn load(journal:&Path)->Result<Self,SystemError>{
        use std::io::Read;
        let mut bytes=Vec::new();crate::directory_guard::open_record(journal).map_err(error)?.take(16385).read_to_end(&mut bytes).map_err(error)?;
        if bytes.len()>16384{return Err(error("Power journal too large"))}
        let record:Journal=serde_json::from_slice(&bytes).map_err(error)?;
        if record.schema!=1{return Err(error("Unsupported power journal schema"))}
        Ok(Self{lease:Lease{backend:Native,path:journal.to_path_buf(),journal:record}})
    }
    pub fn phase(&self)->Phase{self.lease.journal.phase}
}
struct Native;
#[cfg(windows)]mod platform {
    use super::*;use std::ffi::c_void;
    #[link(name="powrprof")]extern "system"{
        fn PowerGetActiveScheme(root:*mut c_void,guid:*mut *mut SchemeId)->u32;
        fn PowerReadACValueIndex(root:*mut c_void,scheme:*const SchemeId,sub:*const SchemeId,setting:*const SchemeId,value:*mut u32)->u32;
        fn PowerReadDCValueIndex(root:*mut c_void,scheme:*const SchemeId,sub:*const SchemeId,setting:*const SchemeId,value:*mut u32)->u32;
        fn PowerWriteACValueIndex(root:*mut c_void,scheme:*const SchemeId,sub:*const SchemeId,setting:*const SchemeId,value:u32)->u32;
        fn PowerWriteDCValueIndex(root:*mut c_void,scheme:*const SchemeId,sub:*const SchemeId,setting:*const SchemeId,value:u32)->u32;
        fn PowerSetActiveScheme(root:*mut c_void,scheme:*const SchemeId)->u32;
    }
    #[link(name="kernel32")]extern "system"{fn LocalFree(memory:*mut c_void)->*mut c_void;}
    fn check(code:u32)->Result<(),SystemError>{if code==0{Ok(())}else{Err(error(std::io::Error::from_raw_os_error(code as i32)))}}
    impl Backend for Native {
        fn active(&mut self)->Result<SchemeId,SystemError>{
            let mut value=std::ptr::null_mut();check(unsafe{PowerGetActiveScheme(std::ptr::null_mut(),&mut value)})?;
            if value.is_null(){return Err(error("PowerGetActiveScheme returned no GUID"))}
            let guid=unsafe{*value};unsafe{LocalFree(value.cast())};Ok(guid)
        }
        fn read(&mut self,scheme:SchemeId)->Result<PowerSnapshot,SystemError>{
            let mut result=PowerSnapshot{scheme,ac:[0;6],dc:[0;6]};
            for(index,setting)in SETTINGS.into_iter().enumerate(){let(sub,id)=ids(setting);
                check(unsafe{PowerReadACValueIndex(std::ptr::null_mut(),&scheme,&sub,&id,&mut result.ac[index])})?;
                check(unsafe{PowerReadDCValueIndex(std::ptr::null_mut(),&scheme,&sub,&id,&mut result.dc[index])})?;
            }Ok(result)
        }
        fn write(&mut self,scheme:SchemeId,dc:bool,index:usize,value:u32)->Result<(),SystemError>{
            let setting=*SETTINGS.get(index).ok_or_else(||error("Unknown power setting index"))?;let(sub,id)=ids(setting);
            check(unsafe{if dc{PowerWriteDCValueIndex(std::ptr::null_mut(),&scheme,&sub,&id,value)}else{PowerWriteACValueIndex(std::ptr::null_mut(),&scheme,&sub,&id,value)}})
        }
        fn activate(&mut self,scheme:SchemeId)->Result<(),SystemError>{check(unsafe{PowerSetActiveScheme(std::ptr::null_mut(),&scheme)})}
    }
}
#[cfg(not(windows))]impl Backend for Native{
    fn active(&mut self)->Result<SchemeId,SystemError>{Err(error("Calibration power requires Windows"))}
    fn read(&mut self,_:SchemeId)->Result<PowerSnapshot,SystemError>{Err(error("Calibration power requires Windows"))}
    fn write(&mut self,_:SchemeId,_:bool,_:usize,_:u32)->Result<(),SystemError>{Err(error("Calibration power requires Windows"))}
    fn activate(&mut self,_:SchemeId)->Result<(),SystemError>{Err(error("Calibration power requires Windows"))}
}

#[cfg(test)]mod tests {
    use super::*;
    fn scheme()->SchemeId{"48c2b03a-8e61-4837-a3ce-621f85306ee3".to_string().try_into().unwrap()}
    fn original()->PowerSnapshot{PowerSnapshot{scheme:scheme(),ac:[2,5,7,43200,0,18000],dc:[2,5,7,10800,7200,7200]}}
    struct Fake{state:PowerSnapshot,active:SchemeId,writes:usize,fail:Option<usize>,activations:usize}
    impl Fake{fn new()->Self{Self{state:original(),active:scheme(),writes:0,fail:None,activations:0}}}
    impl Backend for Fake {
        fn active(&mut self)->Result<SchemeId,SystemError>{Ok(self.active)}
        fn read(&mut self,id:SchemeId)->Result<PowerSnapshot,SystemError>{assert_eq!(id,self.state.scheme);Ok(self.state.clone())}
        fn write(&mut self,id:SchemeId,dc:bool,index:usize,value:u32)->Result<(),SystemError>{
            assert_eq!(id,self.state.scheme);self.writes+=1;if self.fail==Some(self.writes){return Err(error("injected power write failure"))}
            if dc{self.state.dc[index]=value}else{self.state.ac[index]=value}Ok(())
        }
        fn activate(&mut self,id:SchemeId)->Result<(),SystemError>{self.activations+=1;self.active=id;Ok(())}
    }
    fn directory()->PathBuf{
        let root=std::env::temp_dir().join(format!("acer-power-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        fs::create_dir(&root).unwrap();root
    }
    #[test]fn temporary_and_exact_ac_dc_restore_with_durable_phases(){
        let root=directory();let path=root.join("journal.json");let mut lease=Lease::prepare(Fake::new(),path.clone()).unwrap();
        assert_eq!(lease.backend.writes,0);assert!(lease.apply().unwrap().applied);
        assert_eq!(lease.backend.state.ac,TEMPORARY);assert_eq!(lease.backend.state.dc,TEMPORARY);
        assert!(lease.restore().complete);assert_eq!(lease.backend.state,original());assert_eq!(lease.backend.writes,24);
        let journal:Journal=serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();assert_eq!(journal.phase,Phase::Restored);
        assert!(!lease.restore().attempted);assert_eq!(lease.backend.writes,24);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]fn partial_apply_failure_restores_all_twelve_values(){
        let root=directory();let mut backend=Fake::new();backend.fail=Some(5);
        let mut lease=Lease::prepare(backend,root.join("journal.json")).unwrap();
        let report=lease.apply().unwrap();assert!(!report.applied);assert!(report.rollback.unwrap().complete);
        assert_eq!(lease.backend.writes,17);assert_eq!(lease.backend.state,original());fs::remove_dir_all(root).unwrap();
    }
    #[test]fn later_user_scheme_is_not_reactivated_during_restore(){
        let root=directory();let mut lease=Lease::prepare(Fake::new(),root.join("journal.json")).unwrap();lease.apply().unwrap();
        let later="381b4222-f694-41f0-9685-ff5bb260df2e".to_string().try_into().unwrap();lease.backend.active=later;
        let count=lease.backend.activations;assert!(lease.restore().complete);assert_eq!(lease.backend.active,later);
        assert_eq!(lease.backend.activations,count);assert_eq!(lease.backend.state,original());fs::remove_dir_all(root).unwrap();
    }
    #[test]fn drift_and_existing_journal_never_overwrite_original_or_settings(){
        let root=directory();let path=root.join("journal.json");let mut lease=Lease::prepare(Fake::new(),path.clone()).unwrap();
        let saved=fs::read(&path).unwrap();assert!(Lease::prepare(Fake::new(),path.clone()).is_err());assert_eq!(fs::read(&path).unwrap(),saved);
        lease.backend.state.ac[0]=3;assert!(lease.apply().is_err());assert_eq!(lease.backend.writes,0);
        let cancelled=lease.restore();assert!(!cancelled.attempted);assert!(cancelled.complete);assert_eq!(lease.backend.state.ac[0],3);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]fn failed_restore_attempts_every_value_and_keeps_recovery_required(){
        let root=directory();let path=root.join("journal.json");let mut lease=Lease::prepare(Fake::new(),path.clone()).unwrap();lease.apply().unwrap();
        lease.backend.fail=Some(13);let report=lease.restore();assert!(!report.complete);assert_eq!(report.readback_matches,Some(false));
        assert_eq!(lease.backend.writes,24);assert!(!report.errors.is_empty());
        let journal:Journal=serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();assert_eq!(journal.phase,Phase::RecoveryRequired);
        assert!(lease.restore().complete);assert_eq!(lease.backend.state,original());fs::remove_dir_all(root).unwrap();
    }
    #[test]fn guid_layout_and_fixed_reserve_mapping(){
        assert_eq!(std::mem::size_of::<SchemeId>(),16);assert_eq!(String::from(scheme()),"48c2b03a-8e61-4837-a3ce-621f85306ee3");
        assert_eq!(String::from(ids(Setting::ReserveLevel).1),"f3c5027d-cd16-4930-aa6b-90db844a8f00");
        for invalid in ["", "../escape", "00000000-0000-0000-0000-000000000000"]{assert!(SchemeId::try_from(invalid.to_string()).is_err())}
    }
    #[test]fn interrupted_save_residue_does_not_block_loaded_recovery(){
        let root=directory();let path=root.join("journal.json");
        let mut lease=Lease::prepare(Fake::new(),path.clone()).unwrap();assert!(lease.apply().unwrap().applied);
        let residue=path.with_extension("pending");fs::write(&residue,b"interrupted save").unwrap();
        let journal:Journal=serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        let mut recovered=Lease{backend:lease.backend,path:path.clone(),journal};
        assert!(recovered.restore().complete);assert_eq!(recovered.backend.state,original());
        let saved:Journal=serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();assert_eq!(saved.phase,Phase::Restored);
        assert_eq!(fs::read(&residue).unwrap(),b"interrupted save");
        assert!(!recovered.restore().attempted);assert_eq!(recovered.backend.writes,24);
        assert_eq!(fs::read_dir(&root).unwrap().count(),2);fs::remove_dir_all(root).unwrap();
    }
}
