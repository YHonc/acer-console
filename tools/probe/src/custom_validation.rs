use acer_device::Device;
use acer_protocol::{custom_fan_readback,encode,supervised_custom_ack,verified_fan_ack,verified_fan_readback,verified_fan_ready,ApplyReport,ApplyStatus,Capabilities,Command,DeviceError,FanMode,Snapshot};
use serde::Serialize;
use crate::validation::Outcome;

#[derive(Serialize)]pub struct Report{
    command:&'static str,cpu_percent:u8,gpu_percent:u8,outcome:Outcome,message:String,original_mode:Option<FanMode>,
    capabilities:Option<Capabilities>,initial_snapshot:Option<Snapshot>,trial_apply:Option<ApplyReport>,custom_snapshot:Option<Snapshot>,custom_readback_confirmed:bool,
    restore_attempted:bool,restore_apply:Option<ApplyReport>,restored_snapshot:Option<Snapshot>,restore_readback_confirmed:bool,errors:Vec<(&'static str,DeviceError)>,
}
impl Report{
    pub fn success(&self)->bool{self.outcome==Outcome::ReadbacksConfirmed}
    fn new()->Self{Self{command:"--validate-fan-custom-restore",cpu_percent:70,gpu_percent:70,outcome:Outcome::Rejected,message:String::new(),original_mode:None,capabilities:None,initial_snapshot:None,trial_apply:None,custom_snapshot:None,custom_readback_confirmed:false,restore_attempted:false,restore_apply:None,restored_snapshot:None,restore_readback_confirmed:false,errors:vec![]}}
    fn finish(mut self,message:&str)->Self{self.message=message.into();self}
}
trait CustomDevice{
    fn capabilities(&mut self)->Result<Capabilities,DeviceError>;
    fn snapshot(&mut self)->Result<Snapshot,DeviceError>;
    fn custom70(&mut self)->Result<ApplyReport,DeviceError>;
    fn restore(&mut self)->Result<ApplyReport,DeviceError>;
    fn settle(&mut self);
}
struct Native<'a>(&'a Device);
impl CustomDevice for Native<'_>{
    fn capabilities(&mut self)->Result<Capabilities,DeviceError>{self.0.capabilities()}
    fn snapshot(&mut self)->Result<Snapshot,DeviceError>{self.0.snapshot()}
    fn custom70(&mut self)->Result<ApplyReport,DeviceError>{self.0.begin_fan_custom_70_validation()}
    fn restore(&mut self)->Result<ApplyReport,DeviceError>{self.0.restore_validation_fan()}
    fn settle(&mut self){std::thread::sleep(std::time::Duration::from_secs(3));}
}
pub fn run(device:&Device)->Report{validate(&mut Native(device))}
fn trial_accepted(report:&ApplyReport)->bool{
    if matches!(report.status,ApplyStatus::Failed|ApplyStatus::Rejected){return false;}
    let calls=encode(&Command::SetFanSpeeds{cpu_percent:70,gpu_percent:70}).expect("fixed valid percentages");
    report.writes.len()==calls.len()&&calls.iter().zip(&report.writes).all(|(call,read)|supervised_custom_ack(call,read)==Some(true))
}
fn validate(device:&mut impl CustomDevice)->Report{
    let mut report=Report::new();
    let caps=match device.capabilities(){Ok(c)=>c,Err(e)=>{report.errors.push(("capabilities",e));return report.finish("Cannot verify target; no write attempted");}};
    let target=caps.matched_model&&acer_device::verified_fan_target(&caps.model,&caps.bios);report.capabilities=Some(caps);if !target{return report.finish("Requires PHN16-71/V1.18; no write attempted");}
    let before=match device.snapshot(){Ok(s)=>s,Err(e)=>{report.errors.push(("initial_read",e));return report.finish("Initial read failed; no write attempted");}};
    let ready=verified_fan_ready(&before);report.original_mode=before.fan_mode;report.initial_snapshot=Some(before);if !ready{return report.finish("Requires original Auto/Max, three agreeing behavior reads and both RPM reads; no write attempted");}
    // No early returns after the trial call. Custom must not prevent restoration
    // through the worker's saved Auto/Max receipt.
    let trial_ok=match device.custom70(){Ok(r)=>{if let Some(m)=r.before.as_ref().and_then(|s|s.fan_mode){report.original_mode=Some(m);}let ok=trial_accepted(&r);report.trial_apply=Some(r);ok},Err(e)=>{report.errors.push(("custom_write",e));false}};
    if trial_ok{device.settle();match device.snapshot(){Ok(s)=>{report.custom_readback_confirmed=custom_fan_readback(&s,70,70);report.custom_snapshot=Some(s);},Err(e)=>report.errors.push(("custom_read",e)),}}
    report.restore_attempted=true;
    let restore_ok=match device.restore(){Ok(r)=>{let ok=!matches!(r.status,ApplyStatus::Failed|ApplyStatus::Rejected)&&r.writes.len()==1&&report.original_mode.is_some_and(|m|verified_fan_ack(&r.writes[0],m)==Some(true));report.restore_apply=Some(r);ok},Err(e)=>{report.errors.push(("restore_write",e));false}};
    match device.snapshot(){Ok(s)=>{report.restore_readback_confirmed=restore_ok&&report.original_mode.is_some_and(|mode|verified_fan_readback(&s,mode));report.restored_snapshot=Some(s);},Err(e)=>report.errors.push(("restored_read",e)),}
    report.outcome=if !report.restore_readback_confirmed{Outcome::RestoreUnconfirmed}else if trial_ok&&report.custom_readback_confirmed&&report.errors.is_empty(){Outcome::ReadbacksConfirmed}else{Outcome::Failed};
    let message=if report.success(){"Custom behavior and both 70% target values confirmed; original Auto/Max readback confirmed. Cached Custom percentages may remain 70 while Auto/Max is active."}else if report.restore_readback_confirmed{"Custom trial failed; original Auto/Max confirmed"}else{"Original fan mode restore remains unconfirmed; inspect saved mode and raw restoration result"};report.finish(message)
}

#[cfg(test)]mod tests{
    use super::*;use acer_protocol::{ErrorKind,RawRead,RawValue,gaming};use std::collections::{BTreeMap,VecDeque};
    fn snapshot(custom:bool)->Snapshot{let mut s:Snapshot=serde_json::from_str(include_str!("../../../crates/acer-protocol/tests/fixtures/phn16-71-elevated-read.json")).unwrap();s.fan_mode=if custom{Some(FanMode::Custom)}else{Some(FanMode::Auto)};s.cpu_fan_percent=Some(if custom{70}else{0});s.gpu_fan_percent=s.cpu_fan_percent;s.cpu_fan_rpm=Some(4000.0);s.gpu_fan_rpm=Some(4000.0);if custom{for r in &mut s.reads{let id=r.input.get("gmInput").and_then(RawValue::as_u64);let raw=match(r.method.as_str(),id){("GetGamingFanBehavior",Some(1))=>Some(0x300),("GetGamingFanBehavior",Some(8))=>Some(0xc000),("GetGamingFanBehavior",Some(9))=>Some(0xc300),("GetGamingFanSpeed",Some(1|4))=>Some(70<<8),_=>None};if let Some(value)=raw{r.output.insert("gmOutput".into(),RawValue::unsigned(value));}}}s}
    fn writes(calls:Vec<acer_protocol::Call>)->Vec<RawRead>{calls.into_iter().map(|c|RawRead{class:c.class,method:c.method,input:c.input,output:BTreeMap::from([("gmOutput".into(),RawValue::unsigned(0))]),error:None}).collect()}
    fn result(custom:bool)->ApplyReport{ApplyReport{status:ApplyStatus::Unconfirmed,message:"fake".into(),before:Some(snapshot(false)),after:None,writes:if custom{writes(encode(&Command::SetFanSpeeds{cpu_percent:70,gpu_percent:70}).unwrap())}else{writes(vec![gaming("SetGamingFanBehavior",0x410009)])}}}
    fn error()->DeviceError{DeviceError{kind:ErrorKind::Transport,message:"fake failure".into()}}
    struct Fake{reads:VecDeque<Result<Snapshot,DeviceError>>,trial:Option<Result<ApplyReport,DeviceError>>,restore:Option<Result<ApplyReport,DeviceError>>,calls:Vec<&'static str>}
    impl Fake{fn new()->Self{Self{reads:[Ok(snapshot(false)),Ok(snapshot(true)),Ok(snapshot(false))].into(),trial:Some(Ok(result(true))),restore:Some(Ok(result(false))),calls:vec![]}}}
    impl CustomDevice for Fake{
        fn capabilities(&mut self)->Result<Capabilities,DeviceError>{Ok(Capabilities{model:"PHN16-71".into(),bios:"V1.18".into(),matched_model:true,items:vec![]})}
        fn snapshot(&mut self)->Result<Snapshot,DeviceError>{self.calls.push("read");self.reads.pop_front().unwrap()}
        fn custom70(&mut self)->Result<ApplyReport,DeviceError>{self.calls.push("custom70");self.trial.take().unwrap()}
        fn restore(&mut self)->Result<ApplyReport,DeviceError>{self.calls.push("restore");self.restore.take().unwrap()}
        fn settle(&mut self){self.calls.push("settle");}
    }
    #[test]fn custom_mode_does_not_block_restore(){let mut f=Fake::new();let r=validate(&mut f);assert!(r.success());assert_eq!(r.custom_snapshot.unwrap().fan_mode,Some(FanMode::Custom));assert_eq!(f.calls,["read","custom70","settle","read","restore","read"]);}
    #[test]fn unknown_or_original_custom_refuses(){let mut f=Fake::new();f.reads[0]=Ok(snapshot(true));let r=validate(&mut f);assert_eq!(r.outcome,Outcome::Rejected);assert_eq!(f.calls,["read"]);}
    #[test]fn failed_partial_write_still_restores(){let mut f=Fake::new();let trial=f.trial.as_mut().unwrap().as_mut().unwrap();trial.writes.truncate(2);trial.status=ApplyStatus::Failed;f.reads.remove(1);let r=validate(&mut f);assert_eq!(r.outcome,Outcome::Failed);assert!(r.restore_readback_confirmed);assert_eq!(f.calls,["read","custom70","restore","read"]);}
    #[test]fn trial_error_still_restores(){let mut f=Fake::new();f.trial=Some(Err(error()));f.reads.remove(1);let r=validate(&mut f);assert_eq!(r.outcome,Outcome::Failed);assert!(r.restore_attempted&&r.restore_readback_confirmed);}
    #[test]fn custom_read_error_still_restores(){let mut f=Fake::new();f.reads[1]=Err(error());let r=validate(&mut f);assert_eq!(r.outcome,Outcome::Failed);assert!(r.restore_readback_confirmed);}
    #[test]fn incorrect_percentage_fails_even_with_custom_behavior(){let mut f=Fake::new();let s=f.reads[1].as_mut().unwrap();s.reads.iter_mut().find(|r|r.method=="GetGamingFanSpeed").unwrap().output.insert("gmOutput".into(),RawValue::unsigned(69<<8));let r=validate(&mut f);assert_eq!(r.outcome,Outcome::Failed);assert!(!r.custom_readback_confirmed&&r.restore_readback_confirmed);}
    #[test]fn restore_failure_remains_explicit(){let mut f=Fake::new();f.restore=Some(Err(error()));let r=validate(&mut f);assert_eq!(r.outcome,Outcome::RestoreUnconfirmed);assert!(r.restore_attempted);}
}
