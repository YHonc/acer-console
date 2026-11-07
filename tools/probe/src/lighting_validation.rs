use acer_device::Device;
use acer_protocol::{backlight_frame_ack,backlight_frame_matches,brightness_frame,observed_backlight_frame,ApplyReport,ApplyStatus,BacklightFrame,Capabilities,DeviceError,Snapshot};
use serde::Serialize;
use crate::validation::Outcome;
#[derive(Serialize)]pub struct Report{
    command:&'static str,target_brightness:u8,outcome:Outcome,message:String,capabilities:Option<Capabilities>,original_frame:Option<BacklightFrame>,
    initial_snapshot:Option<Snapshot>,trial_apply:Option<ApplyReport>,brightness_snapshot:Option<Snapshot>,brightness_readback_confirmed:bool,
    restore_attempted:bool,restore_apply:Option<ApplyReport>,restored_snapshot:Option<Snapshot>,restore_readback_confirmed:bool,errors:Vec<(&'static str,DeviceError)>,
}
impl Report{
    pub fn success(&self)->bool{self.outcome==Outcome::ReadbacksConfirmed}
    fn new()->Self{Self{command:"--validate-lighting-brightness-restore",target_brightness:20,outcome:Outcome::Rejected,message:String::new(),capabilities:None,original_frame:None,initial_snapshot:None,trial_apply:None,brightness_snapshot:None,brightness_readback_confirmed:false,restore_attempted:false,restore_apply:None,restored_snapshot:None,restore_readback_confirmed:false,errors:vec![]}}
    fn finish(mut self,message:&str)->Self{self.message=message.into();self}
}
trait LightingDevice{
    fn capabilities(&mut self)->Result<Capabilities,DeviceError>;
    fn snapshot(&mut self)->Result<Snapshot,DeviceError>;
    fn brightness20(&mut self)->Result<ApplyReport,DeviceError>;
    fn restore(&mut self)->Result<ApplyReport,DeviceError>;
    fn settle(&mut self);
}
struct Native<'a>(&'a Device);
impl LightingDevice for Native<'_>{
    fn capabilities(&mut self)->Result<Capabilities,DeviceError>{self.0.capabilities()}
    fn snapshot(&mut self)->Result<Snapshot,DeviceError>{self.0.snapshot()}
    fn brightness20(&mut self)->Result<ApplyReport,DeviceError>{self.0.begin_lighting_brightness_validation()}
    fn restore(&mut self)->Result<ApplyReport,DeviceError>{self.0.restore_validation_lighting()}
    fn settle(&mut self){std::thread::sleep(std::time::Duration::from_secs(2));}
}
pub fn run(device:&Device)->Report{validate(&mut Native(device))}
fn accepted(report:&ApplyReport,frame:&[u8])->bool{!matches!(report.status,ApplyStatus::Failed|ApplyStatus::Rejected)&&report.writes.len()==1&&backlight_frame_ack(&report.writes[0],frame)==Some(true)}
fn validate(device:&mut impl LightingDevice)->Report{
    let mut report=Report::new();
    let caps=match device.capabilities(){Ok(c)=>c,Err(e)=>{report.errors.push(("capabilities",e));return report.finish("Cannot verify target; no lighting write attempted");}};
    let target=caps.matched_model&&acer_device::verified_fan_target(&caps.model,&caps.bios);report.capabilities=Some(caps);if !target{return report.finish("Requires PHN16-71/V1.18; no write attempted");}
    let before=match device.snapshot(){Ok(s)=>s,Err(e)=>{report.errors.push(("initial_read",e));return report.finish("Initial read failed; no write attempted");}};
    report.original_frame=observed_backlight_frame(&before);report.initial_snapshot=Some(before);if report.original_frame.is_none(){return report.finish("Requires static0, complete getter15 plus independent gmReturn0 and four readable zones; no write attempted");}
    // From the first possible write onward, restoration is attempted on every
    // Result failure, without requiring the current effect to remain static.
    let trial_ok=match device.brightness20(){Ok(r)=>{if let Some(frame)=r.before.as_ref().and_then(observed_backlight_frame){report.original_frame=Some(frame);}let expected=brightness_frame(report.original_frame.as_ref().unwrap(),20).unwrap();let ok=accepted(&r,&expected);report.trial_apply=Some(r);ok},Err(e)=>{report.errors.push(("brightness_write",e));false}};
    if trial_ok{device.settle();match device.snapshot(){Ok(s)=>{report.brightness_readback_confirmed=backlight_frame_matches(&s,report.original_frame.as_ref().unwrap(),20);report.brightness_snapshot=Some(s);},Err(e)=>report.errors.push(("brightness_read",e)),}}
    report.restore_attempted=true;
    let saved=report.original_frame.as_ref().unwrap();
    let restore_ok=match device.restore(){Ok(r)=>{let ok=accepted(&r,&saved.setter_frame);report.restore_apply=Some(r);ok},Err(e)=>{report.errors.push(("restore_write",e));false}};
    match device.snapshot(){Ok(s)=>{report.restore_readback_confirmed=restore_ok&&backlight_frame_matches(&s,saved,saved.observed_state[2]);report.restored_snapshot=Some(s);},Err(e)=>report.errors.push(("restored_read",e)),}
    report.outcome=if !report.restore_readback_confirmed{Outcome::RestoreUnconfirmed}else if trial_ok&&report.brightness_readback_confirmed&&report.errors.is_empty(){Outcome::ReadbacksConfirmed}else{Outcome::Failed};
    let message=if report.success(){"Brightness20 and saved original brightness confirmed; normalized remaining state and four zone words unchanged"}else if report.restore_readback_confirmed{"Brightness trial failed; saved original state and four zones confirmed"}else{"Original lighting restore remains unconfirmed; inspect saved getter15 and reconstructed setter16 frames"};report.finish(message)
}
#[cfg(test)]mod tests{
    use super::*;use acer_protocol::{backlight_frame_call,ErrorKind,RawRead,RawValue};use std::collections::{BTreeMap,VecDeque};
    fn snapshot(brightness:u8)->Snapshot{let mut s:Snapshot=serde_json::from_str(include_str!("../../../crates/acer-protocol/tests/fixtures/phn16-71-elevated-read.json")).unwrap();let read=s.reads.iter_mut().find(|r|r.method=="GetGamingKBBacklight").unwrap();let Some(RawValue::Bytes(state))=read.output.get_mut("gmOutput")else{panic!()};state[2]=brightness;s}
    fn applied(brightness:u8)->ApplyReport{let saved=observed_backlight_frame(&snapshot(0)).unwrap();let call=backlight_frame_call(brightness_frame(&saved,brightness).unwrap());ApplyReport{status:ApplyStatus::Unconfirmed,message:"fake".into(),before:Some(snapshot(0)),after:None,writes:vec![RawRead{class:call.class,method:call.method,input:call.input,output:BTreeMap::from([("gmOutput".into(),RawValue::unsigned(0))]),error:None}]}}
    fn error()->DeviceError{DeviceError{kind:ErrorKind::Transport,message:"fake failure".into()}}
    struct Fake{reads:VecDeque<Result<Snapshot,DeviceError>>,trial:Option<Result<ApplyReport,DeviceError>>,restore:Option<Result<ApplyReport,DeviceError>>,calls:Vec<&'static str>}
    impl Fake{fn new()->Self{Self{reads:[Ok(snapshot(0)),Ok(snapshot(20)),Ok(snapshot(0))].into(),trial:Some(Ok(applied(20))),restore:Some(Ok(applied(0))),calls:vec![]}}}
    impl LightingDevice for Fake{
        fn capabilities(&mut self)->Result<Capabilities,DeviceError>{Ok(Capabilities{model:"PHN16-71".into(),bios:"V1.18".into(),matched_model:true,items:vec![]})}
        fn snapshot(&mut self)->Result<Snapshot,DeviceError>{self.calls.push("read");self.reads.pop_front().unwrap()}
        fn brightness20(&mut self)->Result<ApplyReport,DeviceError>{self.calls.push("brightness20");self.trial.take().unwrap()}
        fn restore(&mut self)->Result<ApplyReport,DeviceError>{self.calls.push("restore");self.restore.take().unwrap()}
        fn settle(&mut self){self.calls.push("settle");}
    }
    #[test]fn reconstructed_frame_trial_and_original_restore_are_recorded(){let mut f=Fake::new();let r=validate(&mut f);assert!(r.success());let frame=r.original_frame.unwrap();assert_eq!((frame.observed_state.len(),frame.setter_frame.len()),(15,16));assert!(frame.reconstructed);assert_eq!(f.calls,["read","brightness20","settle","read","restore","read"]);}
    #[test]fn dynamic_or_missing_zone_refuses_without_write(){for dynamic in [false,true]{let mut f=Fake::new();let s=f.reads[0].as_mut().unwrap();if dynamic{let read=s.reads.iter_mut().find(|r|r.method=="GetGamingKBBacklight").unwrap();if let Some(RawValue::Bytes(bytes))=read.output.get_mut("gmOutput"){bytes[0]=3;}}else{s.reads.retain(|r|!(r.method=="GetGamingRgbKb"&&r.input.get("gmInput").and_then(RawValue::as_u64)==Some(8)));}let r=validate(&mut f);assert_eq!(r.outcome,Outcome::Rejected);assert_eq!(f.calls,["read"]);}}
    #[test]fn write_error_always_attempts_saved_restore(){let mut f=Fake::new();f.trial=Some(Err(error()));f.reads.remove(1);let r=validate(&mut f);assert_eq!(r.outcome,Outcome::Failed);assert!(r.restore_attempted&&r.restore_readback_confirmed);}
    #[test]fn read_error_always_attempts_saved_restore(){let mut f=Fake::new();f.reads[1]=Err(error());let r=validate(&mut f);assert_eq!(r.outcome,Outcome::Failed);assert!(r.restore_readback_confirmed);}
    #[test]fn changed_zone_fails_trial_but_still_restores(){let mut f=Fake::new();let read=f.reads[1].as_mut().unwrap().reads.iter_mut().find(|r|r.method=="GetGamingRgbKb").unwrap();read.output.insert("gmOutput".into(),RawValue::unsigned(0));let r=validate(&mut f);assert_eq!(r.outcome,Outcome::Failed);assert!(!r.brightness_readback_confirmed&&r.restore_readback_confirmed);}
    #[test]fn failed_restore_is_explicit(){let mut f=Fake::new();f.restore=Some(Err(error()));let r=validate(&mut f);assert_eq!(r.outcome,Outcome::RestoreUnconfirmed);assert!(r.restore_attempted);}
}
