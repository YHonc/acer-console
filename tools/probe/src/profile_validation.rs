use acer_device::Device;
use acer_protocol::{profile_trial_ready, ApplyReport, ApplyStatus, Capabilities, Command, DeviceError, Profile, Snapshot};
use serde::Serialize;
use crate::validation::Outcome;

#[derive(Serialize)]
pub struct Report {
    command: &'static str, target_profile:Profile, outcome: Outcome, message: String, original_profile: Option<Profile>,
    capabilities: Option<Capabilities>, initial_snapshot: Option<Snapshot>, trial_apply: Option<ApplyReport>,
    target_snapshot: Option<Snapshot>, target_readback_confirmed: bool, restore_attempted: bool,
    restore_apply: Option<ApplyReport>, restored_snapshot: Option<Snapshot>, restore_readback_confirmed: bool,
    errors: Vec<(&'static str, DeviceError)>,
}
impl Report {
    pub fn success(&self)->bool { self.outcome==Outcome::ReadbacksConfirmed }
    fn new(target:Profile)->Self {Self{command:if target==Profile::Balanced{"--validate-profile-balanced-restore"}else{"--validate-profile"},target_profile:target,outcome:Outcome::Rejected,message:String::new(),original_profile:None,capabilities:None,initial_snapshot:None,trial_apply:None,target_snapshot:None,target_readback_confirmed:false,restore_attempted:false,restore_apply:None,restored_snapshot:None,restore_readback_confirmed:false,errors:vec![]}}
    fn finish(mut self,message:&str)->Self{self.message=message.into();self}
}
trait ProfileDevice {
    fn capabilities(&mut self)->Result<Capabilities,DeviceError>;
    fn snapshot(&mut self)->Result<Snapshot,DeviceError>;
    fn set_profile(&mut self,target:Profile)->Result<ApplyReport,DeviceError>;
    fn restore(&mut self)->Result<ApplyReport,DeviceError>;
    fn settle(&mut self);
}
struct Native<'a>(&'a Device);
impl ProfileDevice for Native<'_>{
    fn capabilities(&mut self)->Result<Capabilities,DeviceError>{self.0.capabilities()}
    fn snapshot(&mut self)->Result<Snapshot,DeviceError>{self.0.snapshot()}
    fn set_profile(&mut self,target:Profile)->Result<ApplyReport,DeviceError>{self.0.apply(Command::SetProfile{profile:target})}
    fn restore(&mut self)->Result<ApplyReport,DeviceError>{self.0.restore_validation_profile()}
    fn settle(&mut self){std::thread::sleep(std::time::Duration::from_secs(3));}
}
pub fn run(device:&Device)->Report{validate(&mut Native(device),Profile::Balanced)}
pub fn run_target(device:&Device,target:Profile)->Report{validate(&mut Native(device),target)}
fn accepted(report:&ApplyReport)->bool{!matches!(report.status,ApplyStatus::Failed|ApplyStatus::Rejected)&&!report.writes.is_empty()}
fn validate(device:&mut impl ProfileDevice,target:Profile)->Report{
    let mut report=Report::new(target);
    if !matches!(target,Profile::Balanced|Profile::Quiet|Profile::Turbo){return report.finish("Only Balanced/Quiet/Turbo are allowed supervised targets");}
    let caps=match device.capabilities(){Ok(c)=>c,Err(e)=>{report.errors.push(("capabilities",e));return report.finish("Cannot verify device; no profile write attempted");}};
    let target_device=acer_device::verified_fan_target(&caps.model,&caps.bios)&&caps.matched_model;report.capabilities=Some(caps);
    if !target_device{return report.finish("Requires PHN16-71/V1.18; no write attempted");}
    let before=match device.snapshot(){Ok(s)=>s,Err(e)=>{report.errors.push(("initial_read",e));return report.finish("Initial read failed; no write attempted");}};
    let ready=profile_trial_ready(&before,target);report.original_profile=before.profile;report.initial_snapshot=Some(before);
    if !ready{return report.finish("Requires supported non-Eco original/target modes, AC online and battery >=40%; no write attempted");}
    // Once a trial is attempted, no early return may bypass restore.
    let trial_ok=match device.set_profile(target){Ok(r)=>{if let Some(p)=r.before.as_ref().and_then(|s|s.profile){report.original_profile=Some(p);}let ok=accepted(&r);report.trial_apply=Some(r);ok},Err(e)=>{report.errors.push(("target_write",e));false}};
    if trial_ok{device.settle();match device.snapshot(){Ok(s)=>{report.target_readback_confirmed=s.profile==Some(target);report.target_snapshot=Some(s);},Err(e)=>report.errors.push(("target_read",e)),}}
    report.restore_attempted=true;
    let restore_ok=match device.restore(){Ok(r)=>{let ok=accepted(&r);report.restore_apply=Some(r);ok},Err(e)=>{report.errors.push(("restore_write",e));false}};
    match device.snapshot(){Ok(s)=>{report.restore_readback_confirmed=restore_ok&&s.profile==report.original_profile;report.restored_snapshot=Some(s);},Err(e)=>report.errors.push(("restored_read",e)),}
    report.outcome=if !report.restore_readback_confirmed{Outcome::RestoreUnconfirmed}else if trial_ok&&report.target_readback_confirmed&&report.errors.is_empty(){Outcome::ReadbacksConfirmed}else{Outcome::Failed};
    let message=if report.success(){"Target and original profile readbacks confirmed for this supervised run"}else if report.restore_readback_confirmed{"Trial failed; original profile readback confirmed"}else{"Original profile restore is unconfirmed; inspect saved mode and power conditions before continuing"};report.finish(message)
}

#[cfg(test)]mod tests{
    use super::*;use acer_protocol::{ErrorKind,RawRead,RawValue};use std::collections::{VecDeque,BTreeMap};
    fn snapshot(profile:Profile)->Snapshot{let mut s:Snapshot=serde_json::from_str(include_str!("../../../crates/acer-protocol/tests/fixtures/phn16-71-elevated-read.json")).unwrap();s.profile=Some(profile);s}
    fn result()->ApplyReport{ApplyReport{status:ApplyStatus::Unconfirmed,message:"fake".into(),before:Some(snapshot(Profile::Performance)),after:None,writes:vec![RawRead{class:"AcerGamingFunction".into(),method:"SetGamingMiscSetting".into(),input:BTreeMap::new(),output:BTreeMap::from([("gmOutput".into(),RawValue::unsigned(0))]),error:None}]}}
    fn error()->DeviceError{DeviceError{kind:ErrorKind::Transport,message:"fake failure".into()}}
    struct Fake{reads:VecDeque<Result<Snapshot,DeviceError>>,trial:Option<Result<ApplyReport,DeviceError>>,restore:Option<Result<ApplyReport,DeviceError>>,calls:Vec<&'static str>}
    impl Fake{fn new()->Self{Self{reads:[Ok(snapshot(Profile::Performance)),Ok(snapshot(Profile::Balanced)),Ok(snapshot(Profile::Performance))].into(),trial:Some(Ok(result())),restore:Some(Ok(result())),calls:vec![]}}}
    impl ProfileDevice for Fake{
        fn capabilities(&mut self)->Result<Capabilities,DeviceError>{Ok(Capabilities{model:"Predator PHN16-71".into(),bios:"V1.18".into(),matched_model:true,items:vec![]})}
        fn snapshot(&mut self)->Result<Snapshot,DeviceError>{self.calls.push("read");self.reads.pop_front().unwrap()}
        fn set_profile(&mut self,target:Profile)->Result<ApplyReport,DeviceError>{self.calls.push(match target{Profile::Quiet=>"quiet",Profile::Turbo=>"turbo",_=>"balanced"});self.trial.take().unwrap()}
        fn restore(&mut self)->Result<ApplyReport,DeviceError>{self.calls.push("restore");self.restore.take().unwrap()}
        fn settle(&mut self){self.calls.push("settle");}
    }
    #[test]fn balanced_and_original_are_read_back(){let mut f=Fake::new();assert!(validate(&mut f,Profile::Balanced).success());assert_eq!(f.calls,["read","balanced","settle","read","restore","read"]);}
    #[test]fn eco_dc_low_or_unknown_battery_refuse_without_write(){for kind in 0..4{let mut f=Fake::new();let s=f.reads[0].as_mut().unwrap();match kind{0=>s.profile=Some(Profile::Eco),1=>s.ac_online=Some(false),2=>s.battery_percent=Some(39),_=>s.battery_percent=None};assert_eq!(validate(&mut f,Profile::Balanced).outcome,Outcome::Rejected);assert_eq!(f.calls,["read"]);}}
    #[test]fn trial_failure_always_attempts_restore(){let mut f=Fake::new();f.trial=Some(Err(error()));f.reads.remove(1);let r=validate(&mut f,Profile::Balanced);assert_eq!(r.outcome,Outcome::Failed);assert!(r.restore_attempted&&r.restore_readback_confirmed);}
    #[test]fn read_failure_always_attempts_restore(){let mut f=Fake::new();f.reads[1]=Err(error());let r=validate(&mut f,Profile::Balanced);assert_eq!(r.outcome,Outcome::Failed);assert!(r.restore_readback_confirmed);}
    #[test]fn power_loss_restore_refusal_is_preserved(){let mut f=Fake::new();let mut restore=result();restore.status=ApplyStatus::Rejected;restore.message="AC lost; original mode pending restore".into();restore.writes.clear();f.restore=Some(Ok(restore));f.reads[2]=Ok(snapshot(Profile::Balanced));let r=validate(&mut f,Profile::Balanced);assert_eq!(r.outcome,Outcome::RestoreUnconfirmed);assert!(r.restore_attempted);assert!(!r.restore_readback_confirmed);}
    #[test]fn quiet_and_turbo_remain_supervised_targets_with_restore(){for target in [Profile::Quiet,Profile::Turbo]{let mut f=Fake::new();f.reads[1]=Ok(snapshot(target));let report=validate(&mut f,target);assert!(report.success());assert_eq!(report.target_profile,target);assert!(report.restore_readback_confirmed);}}
    #[test]fn eco_is_not_an_experimental_target(){let mut f=Fake::new();assert_eq!(validate(&mut f,Profile::Eco).outcome,Outcome::Rejected);assert!(f.calls.is_empty());}
}
