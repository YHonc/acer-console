//! Native calibration-only WMI access. Not exposed through ordinary Command or
//! WebView input. Keep on the lifecycle owner's COM thread.
use crate::{platform::Backend,verified_fan_target,WritePolicy};
use acer_protocol::{calibration::{self,Mode,Acknowledgement},battery_read,DeviceError,ErrorKind,RawRead};
use serde::Serialize;

#[derive(Debug,Serialize)]
pub struct Observation {
    pub mode:Option<Mode>,pub ac_online:Option<bool>,pub battery_percent:Option<u8>,pub reads:Vec<RawRead>,
}
#[derive(Debug,Serialize)]
pub struct ControlResult {pub acknowledgement:Acknowledgement,pub write:RawRead,pub after:Observation}
pub struct Hardware {backend:Backend,model:String,bios:String,policy:WritePolicy}
fn rejected(message:&str)->DeviceError{DeviceError{kind:ErrorKind::InvalidArgument,message:message.into()}}
fn permit(policy:WritePolicy,model:&str,bios:&str,enabled:bool,before:Option<&Observation>)->Result<(),DeviceError>{
    if policy!=WritePolicy::AllowUnverified{return Err(rejected("Calibration writes require native supervised policy until hardware acceptance"))}
    if !verified_fan_target(model,bios){return Err(rejected("Calibration requires PHN16-71 / V1.18"))}
    if enabled{
        let before=before.ok_or_else(||rejected("Calibration start requires fresh state"))?;
        let mode=before.mode.as_ref().ok_or_else(||rejected("Calibration mode is unknown"))?;
        if !mode.supported||mode.enabled{return Err(rejected("Calibration is unsupported or already enabled"))}
        if before.ac_online!=Some(true){return Err(rejected("Calibration requires confirmed AC power"))}
        // No evidence yet establishes how concurrent charging modes interact.
        // Preserve them rather than silently disabling them to start calibration.
        if mode.function_status.iter().any(|value|*value!=0){return Err(rejected("Another battery function is active; calibration was not started"))}
    }
    Ok(())
}
impl Hardware{
    pub fn connect(policy:WritePolicy)->Result<Self,DeviceError>{
        let backend=Backend::connect(policy)?;let(model,bios)=backend.identity();Ok(Self{backend,model,bios,policy})
    }
    pub fn observe(&self)->Observation{
        let read=self.backend.call(battery_read());let mode=calibration::mode(&read);
        let(ac_online,battery_percent,mut reads)=self.backend.calibration_power();reads.insert(0,read);
        Observation{mode,ac_online,battery_percent,reads}
    }
    /// Caller owns the durable checkpoint, event subscription and exclusivity.
    /// Stop remains available when getter/AC reads fail, for explicit recovery.
    pub fn control(&self,enabled:bool)->Result<ControlResult,DeviceError>{
        let before=if enabled{Some(self.observe())}else{None};
        permit(self.policy,&self.model,&self.bios,enabled,before.as_ref())?;
        let write=self.backend.call(calibration::control(enabled));
        let acknowledgement=calibration::acknowledgement(&write,enabled);let after=self.observe();
        Ok(ControlResult{acknowledgement,write,after})
    }
}
#[cfg(test)]mod tests{
    use super::*;
    fn idle()->Observation{Observation{mode:Some(Mode{supported:true,enabled:false,function_list:3,function_status:[0;5]}),ac_online:Some(true),battery_percent:Some(20),reads:vec![]}}
    #[test]fn calibration_stays_out_of_verified_policy_and_is_model_bound(){
        for policy in [WritePolicy::ReadOnly,WritePolicy::Verified]{assert!(permit(policy,"PHN16-71","V1.18",false,None).is_err())}
        assert!(permit(WritePolicy::AllowUnverified,"PHN16-72","V1.18",false,None).is_err());
        assert!(permit(WritePolicy::AllowUnverified,"PHN16-71","V1.19",false,None).is_err());
        assert!(permit(WritePolicy::AllowUnverified,"PHN16-71","V1.18",false,None).is_ok());
    }
    #[test]fn start_needs_ac_idle_mode_and_preserves_other_battery_functions(){
        let mut state=idle();let check=|s:&Observation|permit(WritePolicy::AllowUnverified,"PHN16-71","V1.18",true,Some(s));
        assert!(check(&state).is_ok());state.ac_online=None;assert!(check(&state).is_err());state.ac_online=Some(true);
        state.mode.as_mut().unwrap().function_status[0]=1;assert!(check(&state).is_err());state.mode.as_mut().unwrap().function_status[0]=0;
        state.mode.as_mut().unwrap().enabled=true;assert!(check(&state).is_err());state.mode=None;assert!(check(&state).is_err());
    }
}
