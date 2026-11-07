//! Fixed calibration protocol. Encoding a command does not authorize execution;
//! the native lifecycle owner must checkpoint power and own event observation first.
use super::{battery_read, BTreeMap, Call, RawRead, RawValue};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mode {
    pub supported: bool,
    /// A mode bit, never evidence of successful completion.
    pub enabled: bool,
    pub function_list: u8,
    pub function_status: [u8; 5],
}

pub fn mode(read: &RawRead) -> Option<Mode> {
    let expected = battery_read();
    if read.error.is_some() || read.class != expected.class || read.method != expected.method || read.input != expected.input { return None; }
    match read.output.get("ReturnValue") { None | Some(RawValue::Bool(true)) => (), _ => return None }
    let RawValue::Bytes(result) = read.output.get("uReturn")? else { return None };
    if result.as_slice() != [0, 0] { return None; }
    let RawValue::Bytes(status) = read.output.get("uFunctionStatus")? else { return None };
    let status: [u8; 5] = status.as_slice().try_into().ok()?;
    if status[1] > 1 { return None; }
    let list = u8::try_from(read.output.get("uFunctionList")?.as_u64()?).ok()?;
    Some(Mode { supported: list & 2 != 0, enabled: status[1] == 1, function_list: list, function_status: status })
}

/// Deliberately separate from the ordinary desktop Command enum.
pub fn control(enabled: bool) -> Call {
    Call { class: "BatteryControl".into(), method: "SetBatteryHealthControl".into(), input: BTreeMap::from([
        ("uBatteryNo".into(), RawValue::unsigned(1)),
        ("uFunctionMask".into(), RawValue::unsigned(2)),
        ("uFunctionStatus".into(), RawValue::unsigned(u64::from(enabled))),
        ("uReservedIn".into(), RawValue::Bytes(vec![0; 5])),
    ]) }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all="snake_case")]
pub enum Acknowledgement { Accepted, Rejected, Unknown }
pub fn acknowledgement(read: &RawRead, enabled: bool) -> Acknowledgement {
    let expected = control(enabled);
    if read.error.is_some() || read.class != expected.class || read.method != expected.method || read.input != expected.input { return Acknowledgement::Rejected; }
    match read.output.get("ReturnValue") { Some(RawValue::Bool(false)) => return Acknowledgement::Rejected, None | Some(RawValue::Bool(true)) => (), _ => return Acknowledgement::Unknown }
    match read.output.get("uReturn").and_then(RawValue::as_u64) {
        Some(0) => Acknowledgement::Accepted,
        Some(1..=65535) => Acknowledgement::Rejected,
        _ => Acknowledgement::Unknown,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all="snake_case")]
pub enum Event { Completed, Interrupted, Unknown(u8) }
/// The caller must associate events with its live owned session. No task ID is
/// present in APGeEvent, so this decoder alone cannot establish attribution.
pub fn event(detail: &[u8]) -> Option<Event> {
    if !(2..=8).contains(&detail.len()) || detail[0] != 11 { return None; }
    Some(match detail[1] { 0 => Event::Interrupted, 1 => Event::Completed, other => Event::Unknown(other) })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all="snake_case")]
pub enum Phase { Starting, Running, Stopping, Restoring, Finished, RecoveryRequired }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all="snake_case")]
pub enum Outcome { Completed, Cancelled, Interrupted, StartFailed, Unknown }
/// In-memory decision layer. The native owner must persist before effects and
/// associate events with the current subscription/session before forwarding them.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Session {
    phase: Phase,
    outcome: Option<Outcome>,
    hardware_stopped: bool,
    last_event: Option<Event>,
}
impl Session {
    /// A restarted process never resumes the calibration or infers success from
    /// saved flags. A fresh observation must establish whether hardware stopped.
    pub fn for_recovery()->Self{Self{phase:Phase::Restoring,outcome:Some(Outcome::Unknown),hardware_stopped:false,last_event:None}}
    pub fn begin(mode: &Mode, ac_online: Option<bool>, listener_ready: bool, checkpoint_saved: bool) -> Result<Self, &'static str> {
        if !mode.supported || mode.enabled { return Err("Calibration is unsupported or already active"); }
        if ac_online != Some(true) { return Err("AC power must be confirmed"); }
        if !listener_ready || !checkpoint_saved { return Err("Event listener and durable power checkpoint are required"); }
        Ok(Self { phase:Phase::Starting, outcome:None, hardware_stopped:false, last_event:None })
    }
    pub fn phase(&self) -> Phase { self.phase }
    pub fn outcome(&self) -> Option<Outcome> { self.outcome }
    pub fn start_result(&mut self, ack: Acknowledgement, mode: Option<&Mode>) -> Result<(), &'static str> {
        if self.phase != Phase::Starting { return Err("No start is pending"); }
        if ack == Acknowledgement::Accepted && mode.is_some_and(|m|m.supported && m.enabled) {
            self.phase=Phase::Running;
        } else {
            self.outcome=Some(Outcome::StartFailed);
            self.hardware_stopped=mode.is_some_and(|m|!m.enabled);
            self.phase=Phase::Restoring;
        }
        Ok(())
    }
    pub fn request_stop(&mut self) -> Result<(), &'static str> {
        if self.phase != Phase::Running { return Err("No running calibration to stop"); }
        self.phase=Phase::Stopping; Ok(())
    }
    pub fn stop_result(&mut self, ack: Acknowledgement, mode: Option<&Mode>) -> Result<(), &'static str> {
        if self.phase != Phase::Stopping { return Err("No stop is pending"); }
        self.hardware_stopped=mode.is_some_and(|m|!m.enabled);
        self.outcome=Some(if ack==Acknowledgement::Accepted && self.hardware_stopped {Outcome::Cancelled} else {Outcome::Unknown});
        // Restore system settings even if stopping the firmware failed.
        self.phase=Phase::Restoring; Ok(())
    }
    pub fn observe_event(&mut self, event:Event) -> bool {
        if !matches!(self.phase,Phase::Starting|Phase::Running|Phase::Stopping) { return false; }
        self.last_event=Some(event);
        let outcome=match event {Event::Completed=>Outcome::Completed,Event::Interrupted=>Outcome::Interrupted,Event::Unknown(_)=>return false};
        self.outcome=Some(outcome);self.hardware_stopped=true;self.phase=Phase::Restoring;true
    }
    /// AC loss, listener failure, resume, or an unexpected mode-off read cannot
    /// establish success. The owner must attempt stop then restore the checkpoint.
    pub fn abort(&mut self, mode:Option<&Mode>) -> Result<(), &'static str> {
        if matches!(self.phase,Phase::Finished|Phase::RecoveryRequired|Phase::Restoring) { return Err("Cleanup already started"); }
        self.outcome=Some(Outcome::Unknown);self.hardware_stopped=mode.is_some_and(|m|!m.enabled);self.phase=Phase::Restoring;Ok(())
    }
    pub fn cleanup_result(&mut self, power_restored:bool, keep_awake_released:bool, listener_stopped:bool, mode:Option<&Mode>) -> Result<(), &'static str> {
        if !matches!(self.phase,Phase::Restoring|Phase::RecoveryRequired) { return Err("No cleanup is pending"); }
        if let Some(mode)=mode { self.hardware_stopped=!mode.enabled; }
        self.phase=if self.hardware_stopped && power_restored && keep_awake_released && listener_stopped { Phase::Finished } else { Phase::RecoveryRequired };
        Ok(())
    }
}

#[cfg(test)] mod tests {
    use super::*;
    fn response(call: Call, output: BTreeMap<String, RawValue>) -> RawRead { RawRead { class: call.class, method: call.method, input: call.input, output, error: None } }
    #[test] fn mode_requires_exact_getter_and_preserves_unknown_functions() {
        let mut read = response(battery_read(), BTreeMap::from([
            ("uReturn".into(), RawValue::Bytes(vec![0,0])), ("uFunctionList".into(), RawValue::unsigned(3)),
            ("uFunctionStatus".into(), RawValue::Bytes(vec![0,1,9,0,0])),
        ]));
        let state = mode(&read).unwrap(); assert!(state.supported && state.enabled); assert_eq!(state.function_status[2],9);
        read.output.insert("uFunctionStatus".into(), RawValue::Bytes(vec![0,2,0,0,0])); assert!(mode(&read).is_none());
        read.output.insert("uFunctionStatus".into(), RawValue::Bytes(vec![0,0,0,0,0]));
        assert!(!mode(&read).unwrap().enabled);
        read.input.insert("uBatteryNo".into(), RawValue::unsigned(2)); assert!(mode(&read).is_none());
    }
    #[test] fn calibration_mask_is_distinct_and_ack_is_not_completion() {
        let call = control(true); assert_eq!(call.input["uFunctionMask"].as_u64(),Some(2));
        let mut read = response(call, BTreeMap::from([("uReturn".into(),RawValue::unsigned(0))]));
        assert_eq!(acknowledgement(&read,true),Acknowledgement::Accepted);
        assert_eq!(acknowledgement(&read,false),Acknowledgement::Rejected);
        read.output.clear(); assert_eq!(acknowledgement(&read,true),Acknowledgement::Unknown);
        read.output.insert("uReturn".into(),RawValue::unsigned(65536)); assert_eq!(acknowledgement(&read,true),Acknowledgement::Unknown);
        read.output.insert("uReturn".into(),RawValue::unsigned(7)); assert_eq!(acknowledgement(&read,true),Acknowledgement::Rejected);
    }
    #[test] fn event_uses_second_byte_and_does_not_invent_progress() {
        assert_eq!(event(&[11,1,0,0,0,0,0,0]),Some(Event::Completed));
        assert_eq!(event(&[11,0,1]),Some(Event::Interrupted));
        assert_eq!(event(&[11,9]),Some(Event::Unknown(9)));
        for bytes in [&[][..], &[11], &[10,1], &[11,1,0,0,0,0,0,0,0]] { assert!(event(bytes).is_none()); }
    }
    fn idle()->Mode {Mode{supported:true,enabled:false,function_list:3,function_status:[0;5]}}
    fn running()->Session {let mut s=Session::begin(&idle(),Some(true),true,true).unwrap();let mut mode=idle();mode.enabled=true;s.start_result(Acknowledgement::Accepted,Some(&mode)).unwrap();s}
    #[test] fn completion_needs_terminal_event_and_cleanup() {
        let mut s=running();assert_eq!(s.outcome(),None);
        assert!(!s.observe_event(Event::Unknown(7)));assert_eq!(s.phase(),Phase::Running);
        assert!(s.observe_event(Event::Completed));assert_eq!(s.phase(),Phase::Restoring);
        s.cleanup_result(false,true,true,Some(&idle())).unwrap();assert_eq!(s.phase(),Phase::RecoveryRequired);
        s.cleanup_result(true,true,true,Some(&idle())).unwrap();assert_eq!(s.phase(),Phase::Finished);assert_eq!(s.outcome(),Some(Outcome::Completed));
        assert!(!s.observe_event(Event::Interrupted));assert_eq!(s.outcome(),Some(Outcome::Completed));
    }
    #[test] fn failed_stop_still_requires_cleanup_and_keeps_unknown_outcome() {
        let mut s=running();s.request_stop().unwrap();s.stop_result(Acknowledgement::Rejected,None).unwrap();
        assert_eq!(s.phase(),Phase::Restoring);s.cleanup_result(true,true,true,None).unwrap();assert_eq!(s.phase(),Phase::RecoveryRequired);
        s.cleanup_result(true,true,true,Some(&idle())).unwrap();assert_eq!(s.phase(),Phase::Finished);assert_eq!(s.outcome(),Some(Outcome::Unknown));
    }
    #[test] fn cancelled_and_aborted_are_not_success() {
        let mut s=running();s.request_stop().unwrap();s.stop_result(Acknowledgement::Accepted,Some(&idle())).unwrap();s.cleanup_result(true,true,true,None).unwrap();assert_eq!(s.outcome(),Some(Outcome::Cancelled));
        let mut s=running();s.abort(Some(&idle())).unwrap();s.cleanup_result(true,true,true,None).unwrap();assert_eq!(s.outcome(),Some(Outcome::Unknown));
        assert!(Session::begin(&idle(),None,true,true).is_err());assert!(Session::begin(&idle(),Some(true),false,true).is_err());assert!(Session::begin(&idle(),Some(true),true,false).is_err());
    }
}
