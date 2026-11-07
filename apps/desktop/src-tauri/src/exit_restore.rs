//! Native-only temporary fan restore receipt. No UI, elevation, or exit side effects.
use acer_protocol::{ApplyReport, ApplyStatus, Command, FanMode, Snapshot};
use serde::Serialize;
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
pub struct ExitState(Arc<Mutex<Inner>>);
#[derive(Default)]
struct Inner {
    enabled: bool,
    receipt: Option<Result<Command, String>>,
    allow_exit: bool,
    in_progress: bool,
}
#[derive(Debug, Clone, Serialize)]
pub struct PendingRestore {
    pub enabled: bool,
    pub has_receipt: bool,
    pub restore_command: Option<Command>,
    pub blocked_reason: Option<String>,
    pub allow_exit: bool,
    pub in_progress: bool,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordOutcome { Ignored, Captured, AlreadyCaptured, Unrestorable(String) }
#[derive(Debug, Clone, Serialize)]
#[serde(tag="status", rename_all="snake_case")]
pub enum RestoreOutcome {
    NotNeeded,
    Restored { report: ApplyReport },
    Failed { report: Option<ApplyReport>, message: String },
    Blocked { message: String },
    InProgress,
}

/// Only known Auto/Max or an exact pair of Custom setpoints is restorable.
fn restore_command(before: &Snapshot) -> Result<Command, String> {
    match before.fan_mode {
        Some(mode @ (FanMode::Auto | FanMode::Max)) => Ok(Command::SetFanMode {mode}),
        Some(FanMode::Custom) => {
            let (Some(cpu),Some(gpu))=(before.cpu_fan_percent,before.gpu_fan_percent) else {
                return Err("原自定义风扇缺少 CPU/GPU 双百分比，无法开启退出恢复".into());
            };
            if cpu>100 || gpu>100 {return Err("原自定义风扇百分比超出 0..100，无法恢复".into());}
            Ok(Command::SetFanSpeeds {cpu_percent:cpu,gpu_percent:gpu})
        }
        None => Err("原风扇模式未知，无法开启退出恢复".into()),
    }
}
impl ExitState {
    /// Enabling requires a restorable original receipt, or a fresh real snapshot
    /// before the first write. Does not itself create a receipt or write hardware.
    pub fn set_enabled(&self, enabled:bool, current:Option<&Snapshot>)->Result<(),String> {
        let mut inner=self.0.lock().map_err(|e|e.to_string())?;
        if inner.in_progress {return Err("退出恢复正在进行".into());}
        if enabled {
            match &inner.receipt {
                Some(Ok(_))=>{},
                Some(Err(reason))=>return Err(reason.clone()),
                None=>{restore_command(current.ok_or("开启退出恢复需要最新真实风扇快照")?)?;},
            }
        }
        inner.enabled=enabled;inner.allow_exit=false;Ok(())
    }
    /// Call after each fan apply, including Failed/Unconfirmed reports with writes.
    /// Keep the FIRST pre-write state, even if restoration was disabled at that time.
    pub fn record(&self, command:&Command, report:&ApplyReport)->Result<RecordOutcome,String> {
        if !matches!(command,Command::SetFanMode{..}|Command::SetFanSpeeds{..})
            || report.status==ApplyStatus::Rejected
            || !report.writes.iter().any(|w|w.class=="AcerGamingFunction" && matches!(w.method.as_str(),"SetGamingFanBehavior"|"SetGamingFanSpeed")) {
            return Ok(RecordOutcome::Ignored);
        }
        let mut inner=self.0.lock().map_err(|e|e.to_string())?;
        if inner.receipt.is_some() {return Ok(RecordOutcome::AlreadyCaptured);}
        let receipt=report.before.as_ref().ok_or_else(||"首次风扇写入报告缺少 before 快照，无法恢复原状态".to_owned()).and_then(restore_command);
        let outcome=match &receipt {Ok(_)=>RecordOutcome::Captured,Err(message)=>RecordOutcome::Unrestorable(message.clone())};
        inner.receipt=Some(receipt);inner.allow_exit=false;Ok(outcome)
    }
    pub fn pending(&self)->Result<PendingRestore,String> {
        let inner=self.0.lock().map_err(|e|e.to_string())?;
        Ok(PendingRestore {
            enabled:inner.enabled,has_receipt:inner.receipt.is_some(),
            restore_command:inner.receipt.as_ref().and_then(|v|v.as_ref().ok()).cloned(),
            blocked_reason:inner.receipt.as_ref().and_then(|v|v.as_ref().err()).cloned(),
            allow_exit:inner.allow_exit,in_progress:inner.in_progress,
        })
    }
    /// Only call after the user explicitly chooses to exit without restoring.
    /// Receipt remains in memory; this method neither exits nor changes hardware.
    pub fn permit_exit_without_restore(&self)->Result<(),String> {
        let mut inner=self.0.lock().map_err(|e|e.to_string())?;
        if inner.in_progress {return Err("退出恢复正在进行，尚不能跳过".into());}
        inner.allow_exit=true;Ok(())
    }
    /// One synchronous attempt through the existing authorized Verified broker.
    /// Run on a blocking worker. Never launches a helper or retries implicitly.
    pub fn restore(&self, client:&acer_broker::BrokerClient)->Result<RestoreOutcome,String> {
        self.restore_with(|command|client.apply(command))
    }
    fn restore_with(&self, apply:impl FnOnce(Command)->Result<ApplyReport,acer_protocol::DeviceError>)->Result<RestoreOutcome,String> {
        let command={
            let mut inner=self.0.lock().map_err(|e|e.to_string())?;
            if inner.in_progress {return Ok(RestoreOutcome::InProgress);}
            if inner.allow_exit || !inner.enabled || inner.receipt.is_none() {
                inner.allow_exit=true;return Ok(RestoreOutcome::NotNeeded);
            }
            let command=match inner.receipt.as_ref().unwrap() {
                Ok(command)=>command.clone(),Err(message)=>return Ok(RestoreOutcome::Blocked{message:message.clone()}),
            };
            inner.in_progress=true;command
        };
        let result=apply(command);
        let mut inner=self.0.lock().map_err(|e|e.to_string())?;
        inner.in_progress=false;
        match result {
            Ok(report) if report.status==ApplyStatus::Applied=>{
                inner.receipt=None;inner.allow_exit=true;Ok(RestoreOutcome::Restored{report})
            },
            Ok(report)=>{inner.allow_exit=false;Ok(RestoreOutcome::Failed{message:report.message.clone(),report:Some(report)})},
            Err(err)=>{inner.allow_exit=false;Ok(RestoreOutcome::Failed{message:err.to_string(),report:None})},
        }
    }
}

#[cfg(test)] mod tests {
    use super::*;
    use acer_protocol::{RawRead,DeviceError,ErrorKind};
    use std::collections::BTreeMap;
    fn before(mode:FanMode)->Snapshot { Snapshot {captured_at_ms:1,cpu_temperature_c:None,gpu_temperature_c:None,cpu_fan_rpm:Some(2000.0),gpu_fan_rpm:Some(2000.0),cpu_fan_percent:None,gpu_fan_percent:None,profile:None,fan_mode:Some(mode),lighting:None,lighting_effect:None,lighting_config:None,battery_care:None,ac_online:Some(true),battery_percent:Some(80),reads:vec![]} }
    fn report(snapshot:Snapshot,status:ApplyStatus,attempted:bool)->ApplyReport {
        ApplyReport {status,message:"fixture".into(),before:Some(snapshot),after:None,writes:if attempted {vec![RawRead{class:"AcerGamingFunction".into(),method:"SetGamingFanBehavior".into(),input:BTreeMap::new(),output:BTreeMap::new(),error:None}]}else{vec![]}}
    }
    fn max()->Command {Command::SetFanMode{mode:FanMode::Max}}
    #[test] fn only_first_attempted_fan_write_keeps_original() {
        let state=ExitState::default();let first=before(FanMode::Auto);
        assert_eq!(state.record(&max(),&report(first.clone(),ApplyStatus::Rejected,false)).unwrap(),RecordOutcome::Ignored);
        assert!(!state.pending().unwrap().has_receipt);
        assert_eq!(state.record(&max(),&report(first.clone(),ApplyStatus::Unconfirmed,true)).unwrap(),RecordOutcome::Captured);
        assert_eq!(state.record(&max(),&report(before(FanMode::Max),ApplyStatus::Applied,true)).unwrap(),RecordOutcome::AlreadyCaptured);
        assert_eq!(state.pending().unwrap().restore_command,Some(Command::SetFanMode{mode:FanMode::Auto}));
        state.set_enabled(true,None).unwrap();
    }
    #[test] fn custom_requires_both_percentages_and_restores_exact_pair() {
        let state=ExitState::default();let mut original=before(FanMode::Custom);
        original.cpu_fan_percent=Some(40);assert!(state.set_enabled(true,Some(&original)).is_err());
        original.gpu_fan_percent=Some(65);state.set_enabled(true,Some(&original)).unwrap();
        state.record(&max(),&report(original,ApplyStatus::Applied,true)).unwrap();
        let result=state.restore_with(|cmd|{assert_eq!(cmd,Command::SetFanSpeeds{cpu_percent:40,gpu_percent:65});Ok(report(before(FanMode::Auto),ApplyStatus::Applied,true))}).unwrap();
        assert!(matches!(result,RestoreOutcome::Restored{..}));assert!(!state.pending().unwrap().has_receipt);assert!(state.pending().unwrap().allow_exit);
    }
    #[test] fn failures_keep_receipt_and_require_explicit_retry() {
        let state=ExitState::default();let snapshot=before(FanMode::Auto);state.set_enabled(true,Some(&snapshot)).unwrap();
        state.record(&max(),&report(snapshot.clone(),ApplyStatus::Failed,true)).unwrap();
        assert!(matches!(state.restore_with(|_|Err(DeviceError{kind:ErrorKind::Transport,message:"offline".into()})).unwrap(),RestoreOutcome::Failed{..}));
        assert!(state.pending().unwrap().has_receipt);assert!(!state.pending().unwrap().allow_exit);assert!(!state.pending().unwrap().in_progress);
        assert!(matches!(state.restore_with(|_|Ok(report(snapshot,ApplyStatus::Unconfirmed,true))).unwrap(),RestoreOutcome::Failed{..}));
        assert!(state.pending().unwrap().has_receipt);
    }
    #[test] fn incomplete_first_receipt_is_never_replaced_with_later_state() {
        let state=ExitState::default();assert!(matches!(state.record(&max(),&report(before(FanMode::Custom),ApplyStatus::Failed,true)).unwrap(),RecordOutcome::Unrestorable(_)));
        state.record(&max(),&report(before(FanMode::Auto),ApplyStatus::Applied,true)).unwrap();
        assert!(state.set_enabled(true,Some(&before(FanMode::Auto))).is_err());assert!(state.pending().unwrap().blocked_reason.is_some());
    }
    #[test] fn restore_is_single_flight_and_disabled_never_calls_apply() {
        let state=ExitState::default();assert!(matches!(state.restore_with(|_|panic!("disabled must not write")).unwrap(),RestoreOutcome::NotNeeded));
        let snapshot=before(FanMode::Auto);state.set_enabled(true,Some(&snapshot)).unwrap();state.record(&max(),&report(snapshot.clone(),ApplyStatus::Applied,true)).unwrap();
        let other=state.clone();state.restore_with(|_|{
            assert!(other.pending().unwrap().in_progress);
            assert!(matches!(other.restore_with(|_|panic!("must not write twice")).unwrap(),RestoreOutcome::InProgress));
            assert!(other.permit_exit_without_restore().is_err());
            Ok(report(snapshot,ApplyStatus::Applied,true))
        }).unwrap();
    }
}
