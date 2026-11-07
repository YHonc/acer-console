use acer_device::Device;
use acer_protocol::{animation_readback, backlight_frame_matches, dynamic_restore_plan,
    lighting_call_ack, lighting_config_from_reads, observed_backlight_frame,
    observed_lighting_frame, verified_animation_result, verified_lighting_restore_result,
    verified_static_result, ApplyReport, ApplyStatus, BacklightFrame, Capabilities, Command,
    DeviceError, Lighting, LightingAnimation, LightingEffect, Snapshot};
use serde::Serialize;
use crate::validation::Outcome;

#[derive(Serialize)]
pub struct Step {
    name: &'static str,
    command: Command,
    apply: Option<ApplyReport>,
    settled_snapshot: Option<Snapshot>,
    readback_confirmed: bool,
}
#[derive(Serialize)]
pub struct Report {
    command: &'static str,
    outcome: Outcome,
    message: String,
    capabilities: Option<Capabilities>,
    checkpoint: Option<ApplyReport>,
    original_frame: Option<BacklightFrame>,
    steps: Vec<Step>,
    restore_attempted: bool,
    restore_apply: Option<ApplyReport>,
    restored_snapshot: Option<Snapshot>,
    restore_readback_confirmed: bool,
    errors: Vec<(String, DeviceError)>,
}
impl Report {
    pub fn success(&self) -> bool { self.outcome == Outcome::ReadbacksConfirmed }
    fn new() -> Self { Self { command:"--validate-lighting-verified-restore", outcome:Outcome::Rejected,
        message:String::new(), capabilities:None, checkpoint:None, original_frame:None,
        steps:vec![], restore_attempted:false, restore_apply:None, restored_snapshot:None,
        restore_readback_confirmed:false, errors:vec![] } }
    fn finish(mut self, message: &str) -> Self { self.message=message.into(); self }
}
trait VerifiedLightingDevice {
    fn capabilities(&mut self) -> Result<Capabilities, DeviceError>;
    fn checkpoint(&mut self) -> Result<ApplyReport, DeviceError>;
    fn apply(&mut self, command: Command) -> Result<ApplyReport, DeviceError>;
    fn snapshot(&mut self) -> Result<Snapshot, DeviceError>;
    fn restore_checkpoint(&mut self) -> Result<ApplyReport, DeviceError>;
    fn settle(&mut self);
}
struct Native<'a> { checkpoint:&'a Device, verified:&'a Device }
impl VerifiedLightingDevice for Native<'_> {
    fn capabilities(&mut self)->Result<Capabilities,DeviceError>{self.verified.capabilities()}
    fn checkpoint(&mut self)->Result<ApplyReport,DeviceError>{self.checkpoint.save_validation_lighting_checkpoint()}
    fn apply(&mut self,command:Command)->Result<ApplyReport,DeviceError>{self.verified.apply(command)}
    fn snapshot(&mut self)->Result<Snapshot,DeviceError>{self.verified.snapshot()}
    fn restore_checkpoint(&mut self)->Result<ApplyReport,DeviceError>{self.checkpoint.restore_validation_dynamic_lighting()}
    fn settle(&mut self){std::thread::sleep(std::time::Duration::from_secs(2));}
}
pub fn run(checkpoint:&Device, verified:&Device)->Report {
    validate(&mut Native{checkpoint,verified})
}
fn target(effect:LightingEffect,brightness:u8)->LightingAnimation {
    LightingAnimation{effect,speed:3,brightness,direction:1,color:[255,128,0]}
}
fn step_confirmed(command:&Command, applied:&ApplyReport, snapshot:&Snapshot,
    previous_dynamic:Option<&BacklightFrame>) -> bool {
    if applied.status!=ApplyStatus::Applied { return false; }
    match command {
        Command::SetLightingAnimation{lighting} => verified_animation_result(lighting,&applied.writes,Some(snapshot))==ApplyStatus::Applied,
        Command::RestoreLighting{} => previous_dynamic.is_some_and(|saved|
            verified_lighting_restore_result(saved,&applied.writes,Some(snapshot))==ApplyStatus::Applied),
        Command::SetLighting{lighting} => applied.before.as_ref().and_then(observed_lighting_frame)
            .is_some_and(|saved|verified_static_result(lighting,&saved,&applied.writes,Some(snapshot))==ApplyStatus::Applied),
        _ => false,
    }
}
fn validate(device:&mut impl VerifiedLightingDevice)->Report {
    let mut report=Report::new();
    let caps=match device.capabilities(){Ok(c)=>c,Err(e)=>{
        report.errors.push(("capabilities".into(),e));return report.finish("Cannot verify target; no write attempted");}};
    let matched=caps.matched_model&&acer_device::verified_fan_target(&caps.model,&caps.bios);
    report.capabilities=Some(caps);
    if !matched{return report.finish("Requires PHN16-71/V1.18; no write attempted");}
    let checkpoint=match device.checkpoint(){Ok(r)=>r,Err(e)=>{
        report.errors.push(("checkpoint".into(),e));return report.finish("Read-only checkpoint failed; no write attempted");}};
    let saved=checkpoint.before.as_ref().and_then(observed_backlight_frame);
    let original=checkpoint.before.as_ref().and_then(|s|lighting_config_from_reads(&s.reads))
        .and_then(|c|c.zones.map(|zones|Lighting{brightness:c.brightness,zones}));
    let ready=checkpoint.status==ApplyStatus::Applied&&checkpoint.writes.is_empty()
        &&saved.as_ref().is_some_and(|f|dynamic_restore_plan(f).is_some())&&original.is_some();
    report.original_frame=saved.clone();report.checkpoint=Some(checkpoint);
    if !ready{return report.finish("Original static0 frame and all four zones require a read-only checkpoint; no write attempted");}
    let commands=[
        ("static_to_breathing_brightness_0",Command::SetLightingAnimation{lighting:target(LightingEffect::Breathing,0)}),
        ("breathing_brightness_100",Command::SetLightingAnimation{lighting:target(LightingEffect::Breathing,100)}),
        ("dynamic_to_wave",Command::SetLightingAnimation{lighting:target(LightingEffect::Wave,20)}),
        ("restore_previous_breathing_100",Command::RestoreLighting{}),
        ("dynamic_to_original_static",Command::SetLighting{lighting:original.unwrap()}),
    ];
    let mut previous_dynamic=None;
    for(name,command)in commands {
        let mut step=Step{name,command:command.clone(),apply:None,settled_snapshot:None,readback_confirmed:false};
        match device.apply(command.clone()) {
            Ok(applied)=>{
                if name=="dynamic_to_wave"{previous_dynamic=applied.before.as_ref().and_then(observed_lighting_frame);}
                step.apply=Some(applied);
                device.settle();
                match device.snapshot(){
                    Ok(snapshot)=>{
                        step.readback_confirmed=step_confirmed(&command,step.apply.as_ref().unwrap(),&snapshot,previous_dynamic.as_ref());
                        // The restore target must be the preceding breathing100 config,
                        // rather than merely accepting whichever receipt the worker held.
                        if matches!(command,Command::RestoreLighting{}){
                            step.readback_confirmed&=animation_readback(&snapshot,&target(LightingEffect::Breathing,100));
                        }
                        step.settled_snapshot=Some(snapshot);
                    }
                    Err(e)=>report.errors.push((format!("{name}_read"),e)),
                }
            }
            Err(e)=>report.errors.push((name.into(),e)),
        }
        let confirmed=step.readback_confirmed;report.steps.push(step);
        if !confirmed{break;}
    }
    // This independent checkpoint is not the Verified worker's last-operation
    // receipt. Always attempt it after the first possible production write.
    report.restore_attempted=true;
    let saved=saved.unwrap();let calls=dynamic_restore_plan(&saved).unwrap();
    let acked=match device.restore_checkpoint(){
        Ok(applied)=>{
            let ok=!matches!(applied.status,ApplyStatus::Failed|ApplyStatus::Rejected)
                &&applied.writes.len()==calls.len()
                &&calls.iter().zip(&applied.writes).all(|(c,r)|lighting_call_ack(c,r)==Some(true));
            report.restore_apply=Some(applied);ok
        }
        Err(e)=>{report.errors.push(("checkpoint_restore".into(),e));false}
    };
    device.settle();
    match device.snapshot(){
        Ok(snapshot)=>{
            report.restore_readback_confirmed=acked&&backlight_frame_matches(&snapshot,&saved,saved.observed_state[2]);
            report.restored_snapshot=Some(snapshot);
        }
        Err(e)=>report.errors.push(("checkpoint_restored_read".into(),e)),
    }
    report.outcome=if !report.restore_readback_confirmed{Outcome::RestoreUnconfirmed}
        else if report.steps.len()==5&&report.steps.iter().all(|s|s.readback_confirmed)&&report.errors.is_empty(){Outcome::ReadbacksConfirmed}
        else{Outcome::Failed};
    let message=if report.success(){"Verified brightness0/100, static/dynamic transitions and previous dynamic restore confirmed; independent original static frame and four zones restored"}
        else if report.restore_readback_confirmed{"Production-path trial failed or was unconfirmed; independent original static frame and four zones restored"}
        else{"Independent original lighting restoration remains unconfirmed; inspect all six recovery ACKs and full readback"};
    report.finish(message)
}

#[cfg(test)]mod tests {
    use super::*;
    use acer_protocol::{animation_frame,backlight_frame_call,lighting_restore_plan,static_lighting_plan,ErrorKind,RawRead,RawValue};
    use std::collections::BTreeMap;
    fn original()->Snapshot {let mut snapshot:Snapshot=serde_json::from_str(include_str!("../../../crates/acer-protocol/tests/fixtures/phn16-71-elevated-read.json")).unwrap();let config=lighting_config_from_reads(&snapshot.reads).unwrap();snapshot.lighting=Some(Lighting{brightness:config.brightness,zones:config.zones.unwrap()});snapshot.lighting_effect=Some(config.effect);snapshot.lighting_config=Some(config);snapshot}
    fn error()->DeviceError{DeviceError{kind:ErrorKind::Transport,message:"simulated failure".into()}}
    fn ack(calls:Vec<acer_protocol::Call>)->Vec<RawRead>{calls.into_iter().map(|c|RawRead{class:c.class,method:c.method,input:c.input,output:BTreeMap::from([("gmOutput".into(),RawValue::unsigned(0))]),error:None}).collect()}
    fn set_frame(snapshot:&mut Snapshot,frame:Vec<u8>){snapshot.reads.iter_mut().find(|r|r.method=="GetGamingKBBacklight").unwrap().output.insert("gmOutput".into(),RawValue::Bytes(frame[..15].to_vec()));snapshot.lighting_config=lighting_config_from_reads(&snapshot.reads);snapshot.lighting_effect=snapshot.lighting_config.as_ref().map(|c|c.effect);snapshot.lighting=None;}
    struct Fake{current:Snapshot,checkpoint:Snapshot,previous:Option<Snapshot>,apply_count:usize,read_count:usize,fail_step:Option<usize>,fail_read:Option<usize>,bad_ack:bool,truncated_restore:bool,reject_checkpoint:bool,calls:Vec<&'static str>}
    impl Fake{fn new()->Self{Self{current:original(),checkpoint:original(),previous:None,apply_count:0,read_count:0,fail_step:None,fail_read:None,bad_ack:false,truncated_restore:false,reject_checkpoint:false,calls:vec![]}}}
    impl VerifiedLightingDevice for Fake{
        fn capabilities(&mut self)->Result<Capabilities,DeviceError>{Ok(Capabilities{model:"PHN16-71".into(),bios:"V1.18".into(),matched_model:true,items:vec![]})}
        fn checkpoint(&mut self)->Result<ApplyReport,DeviceError>{self.calls.push("checkpoint");Ok(ApplyReport{status:if self.reject_checkpoint{ApplyStatus::Rejected}else{ApplyStatus::Applied},message:"read-only simulated checkpoint".into(),before:Some(self.checkpoint.clone()),after:None,writes:vec![]})}
        fn apply(&mut self,command:Command)->Result<ApplyReport,DeviceError>{
            self.calls.push("apply");let index=self.apply_count;self.apply_count+=1;
            let before=self.current.clone();let saved=observed_lighting_frame(&before).unwrap();
            let calls=match command{
                Command::SetLightingAnimation{lighting}=>{self.previous=Some(before.clone());let frame=animation_frame(&lighting).unwrap();set_frame(&mut self.current,frame.clone());vec![backlight_frame_call(frame)]}
                Command::RestoreLighting{}=>{let previous=self.previous.take().unwrap();let calls=lighting_restore_plan(&observed_lighting_frame(&previous).unwrap()).unwrap();self.current=previous;calls}
                Command::SetLighting{lighting}=>{self.previous=Some(before.clone());let calls=static_lighting_plan(&saved,&lighting).unwrap();self.current=self.checkpoint.clone();calls}
                _=>unreachable!(),
            };
            if self.fail_step==Some(index){return Err(error());}
            let mut writes=ack(calls);if self.bad_ack{writes[0].output.clear();}
            Ok(ApplyReport{status:ApplyStatus::Applied,message:"simulated production result".into(),before:Some(before),after:Some(self.current.clone()),writes})
        }
        fn snapshot(&mut self)->Result<Snapshot,DeviceError>{self.calls.push("read");let index=self.read_count;self.read_count+=1;if self.fail_read==Some(index){Err(error())}else{Ok(self.current.clone())}}
        fn restore_checkpoint(&mut self)->Result<ApplyReport,DeviceError>{self.calls.push("full_restore");let before=self.current.clone();self.current=self.checkpoint.clone();let mut writes=ack(dynamic_restore_plan(&observed_backlight_frame(&self.checkpoint).unwrap()).unwrap());if self.truncated_restore{writes.pop();}Ok(ApplyReport{status:ApplyStatus::Unconfirmed,message:"simulated independent checkpoint recovery".into(),before:Some(before),after:Some(self.current.clone()),writes})}
        fn settle(&mut self){}
    }
    #[test]fn simulated_production_sequence_checks_both_endpoints_and_last_dynamic(){let mut f=Fake::new();let r=validate(&mut f);assert!(r.success(),"{}",serde_json::to_string_pretty(&r).unwrap());assert_eq!(r.steps.len(),5);assert_eq!(r.restore_apply.unwrap().writes.len(),6);assert_eq!(f.calls.first(),Some(&"checkpoint"));assert_eq!(f.calls.iter().filter(|c|**c=="full_restore").count(),1);}
    #[test]fn every_partial_production_failure_still_restores_independent_original(){for step in 0..5{let mut f=Fake::new();f.fail_step=Some(step);let r=validate(&mut f);assert_eq!(r.outcome,Outcome::Failed);assert!(r.restore_attempted&&r.restore_readback_confirmed);assert_eq!(r.steps.len(),step+1);}}
    #[test]fn failed_step_read_stops_trials_but_restores_original(){let mut f=Fake::new();f.fail_read=Some(2);let r=validate(&mut f);assert_eq!(r.steps.len(),3);assert_eq!(r.outcome,Outcome::Failed);assert!(r.restore_readback_confirmed);}
    #[test]fn applied_label_without_ack_is_not_accepted(){let mut f=Fake::new();f.bad_ack=true;let r=validate(&mut f);assert_eq!(r.outcome,Outcome::Failed);assert_eq!(r.steps.len(),1);assert!(r.restore_readback_confirmed);}
    #[test]fn truncated_final_restore_is_unconfirmed_even_when_snapshot_matches(){let mut f=Fake::new();f.truncated_restore=true;let r=validate(&mut f);assert_eq!(r.outcome,Outcome::RestoreUnconfirmed);assert!(!r.restore_readback_confirmed);}
    #[test]fn final_read_failure_remains_restore_unconfirmed(){let mut f=Fake::new();f.fail_read=Some(5);let r=validate(&mut f);assert_eq!(r.outcome,Outcome::RestoreUnconfirmed);assert!(r.restore_attempted);}
    #[test]fn rejected_checkpoint_never_starts_production_writes(){let mut f=Fake::new();f.reject_checkpoint=true;let r=validate(&mut f);assert_eq!(r.outcome,Outcome::Rejected);assert_eq!(f.calls,["checkpoint"]);}
}
