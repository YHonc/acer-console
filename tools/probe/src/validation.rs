use acer_device::Device;
use acer_protocol::{verified_fan_ready,ApplyReport, ApplyStatus, Capabilities, Command, DeviceError, FanMode, Snapshot};
use serde::Serialize;
const SETTLE_MS: u64 = 3_000;

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Outcome { ReadbacksConfirmed, Rejected, Failed, RestoreUnconfirmed }
#[derive(Debug, Serialize)]
pub struct ValidationError { phase: &'static str, error: DeviceError }
#[derive(Debug, Serialize)]
pub struct ValidationReport {
    schema_version: u8, command: &'static str, started_at_ms: u64, finished_at_ms: u64, settle_ms: u64,
    outcome: Outcome, message: String, capabilities: Option<Capabilities>, initial_snapshot: Option<Snapshot>, original_mode: Option<FanMode>,
    max_call_attempted: bool, max_apply: Option<ApplyReport>, max_settled_snapshot: Option<Snapshot>, max_readback_confirmed: bool,
    restore_call_attempted: bool, restore_apply: Option<ApplyReport>, restored_snapshot: Option<Snapshot>, restore_readback_confirmed: bool,
    errors: Vec<ValidationError>,
}
impl ValidationReport {
    pub fn success(&self) -> bool { self.outcome == Outcome::ReadbacksConfirmed }
    fn new() -> Self {
        Self { schema_version: 1, command: "--validate-fan-max-restore", started_at_ms: now(), finished_at_ms: 0,
            settle_ms: SETTLE_MS, outcome: Outcome::Rejected, message: String::new(), capabilities: None,
            initial_snapshot: None, original_mode: None, max_call_attempted: false, max_apply: None,
            max_settled_snapshot: None, max_readback_confirmed: false, restore_call_attempted: false,
            restore_apply: None, restored_snapshot: None, restore_readback_confirmed: false, errors: vec![] }
    }
    fn finish(mut self, message: &str) -> Self { self.finished_at_ms = now(); self.message = message.into(); self }
    fn error(&mut self, phase: &'static str, error: DeviceError) { self.errors.push(ValidationError { phase, error }); }
}
fn now() -> u64 { std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as u64 }

// Fakes exercise every failure path without WMI or sleeping.
trait FanDevice {
    fn capabilities(&mut self) -> Result<Capabilities, DeviceError>;
    fn snapshot(&mut self) -> Result<Snapshot, DeviceError>;
    fn set_max(&mut self) -> Result<ApplyReport, DeviceError>;
    fn restore(&mut self) -> Result<ApplyReport, DeviceError>;
    fn settle(&mut self);
}
struct Native<'a>(&'a Device);
impl FanDevice for Native<'_> {
    fn capabilities(&mut self) -> Result<Capabilities, DeviceError> { self.0.capabilities() }
    fn snapshot(&mut self) -> Result<Snapshot, DeviceError> { self.0.snapshot() }
    fn set_max(&mut self) -> Result<ApplyReport, DeviceError> { self.0.apply(Command::SetFanMode { mode: FanMode::Max }) }
    fn restore(&mut self) -> Result<ApplyReport, DeviceError> { self.0.restore_validation_fan() }
    fn settle(&mut self) { std::thread::sleep(std::time::Duration::from_millis(SETTLE_MS)); }
}
pub fn run(device: &Device) -> ValidationReport { validate(&mut Native(device)) }
fn accepted(report: &ApplyReport) -> bool { !matches!(report.status, ApplyStatus::Failed | ApplyStatus::Rejected) && !report.writes.is_empty() }
fn validate(device: &mut impl FanDevice) -> ValidationReport {
    let mut report = ValidationReport::new();
    let caps = match device.capabilities() {
        Ok(caps) => caps,
        Err(error) => { report.error("capabilities", error); return report.finish("Cannot verify target device; no write attempted"); }
    };
    let matches = caps.matched_model && matches!(caps.model.as_str(), "PHN16-71" | "Predator PHN16-71") && caps.bios == "V1.18";
    report.capabilities = Some(caps);
    if !matches { return report.finish("Requires PHN16-71 / V1.18; no write attempted"); }
    let initial = match device.snapshot() {
        Ok(snapshot) => snapshot,
        Err(error) => { report.error("initial_read", error); return report.finish("Initial read failed; no write attempted"); }
    };
    report.original_mode = initial.fan_mode;
    let ready = verified_fan_ready(&initial);
    report.initial_snapshot = Some(initial);
    if !ready { return report.finish("Original fan mode must be Auto/Max and both RPM reads must succeed; no write attempted"); }

    // No early returns after this point. Always attempt to restore, including a
    // transport error during the Max request or a failure of its readback.
    report.max_call_attempted = true;
    let max_accepted = match device.set_max() {
        Ok(applied) => {
            // The worker saves the closest pre-write state. If another controller
            // changed Auto/Max between reads, that saved state is authoritative.
            if let Some(mode) = applied.before.as_ref().and_then(|s| s.fan_mode) { report.original_mode = Some(mode); }
            let accepted = accepted(&applied); report.max_apply = Some(applied); accepted
        }
        Err(error) => { report.error("max_write", error); false }
    };
    if max_accepted {
        device.settle();
        match device.snapshot() {
            Ok(snapshot) => { report.max_readback_confirmed = snapshot.fan_mode == Some(FanMode::Max); report.max_settled_snapshot = Some(snapshot); }
            Err(error) => report.error("max_settled_read", error),
        }
    }
    report.restore_call_attempted = true;
    let restore_accepted = match device.restore() {
        Ok(restored) => { let accepted = accepted(&restored); report.restore_apply = Some(restored); accepted }
        Err(error) => { report.error("restore_write", error); false }
    };
    match device.snapshot() {
        Ok(snapshot) => { report.restore_readback_confirmed = restore_accepted && snapshot.fan_mode == report.original_mode; report.restored_snapshot = Some(snapshot); }
        Err(error) => report.error("restored_read", error),
    }
    report.outcome = if !report.restore_readback_confirmed { Outcome::RestoreUnconfirmed }
        else if max_accepted && report.max_readback_confirmed && report.errors.is_empty() { Outcome::ReadbacksConfirmed }
        else { Outcome::Failed };
    let message = match report.outcome {
        Outcome::ReadbacksConfirmed => "Max and saved original mode were observed in readback; this records one supervised validation run",
        Outcome::RestoreUnconfirmed => "Restoration was attempted but not confirmed; inspect raw restore result and restore the fan setting before continuing",
        _ => "Max validation failed; restoration to the saved mode was confirmed",
    };
    report.finish(message)
}

#[cfg(test)] mod tests {
    use super::*;
    use acer_protocol::{ErrorKind, RawRead, RawValue};
    use std::collections::{BTreeMap, VecDeque};
    fn snapshot(mode: Option<FanMode>) -> Snapshot {
        let mut value: Snapshot = serde_json::from_str(include_str!("../../../crates/acer-protocol/tests/fixtures/phn16-71-elevated-read.json")).unwrap();
        value.fan_mode = mode; value.cpu_fan_rpm = Some(3829.0); value.gpu_fan_rpm = Some(3750.0); if mode==Some(FanMode::Max){for r in &mut value.reads{if r.method=="GetGamingFanBehavior"{let selector=r.input.get("gmInput").and_then(RawValue::as_u64);if let Some(word)=match selector{Some(1)=>Some(0x200),Some(8)=>Some(0x8000),Some(9)=>Some(0x8200),_=>None}{r.output.insert("gmOutput".into(),RawValue::unsigned(word));}}}}value
    }
    fn applied() -> ApplyReport {
        ApplyReport { status: ApplyStatus::Unconfirmed, message: "mock WMI reply".into(), before: Some(snapshot(Some(FanMode::Auto))), after: None,
            writes: vec![RawRead { class: "AcerGamingFunction".into(), method: "SetGamingFanBehavior".into(), input: BTreeMap::new(), output: BTreeMap::from([("gmOutput".into(), RawValue::unsigned(0))]), error: None }] }
    }
    fn failure() -> DeviceError { DeviceError { kind: ErrorKind::Transport, message: "simulated failure".into() } }
    struct Fake { model: String, reads: VecDeque<Result<Snapshot, DeviceError>>, max: Option<Result<ApplyReport, DeviceError>>, restore_result: Option<Result<ApplyReport, DeviceError>>, calls: Vec<&'static str> }
    impl Fake {
        fn normal() -> Self { Self { model: "Predator PHN16-71".into(), reads: [Ok(snapshot(Some(FanMode::Auto))), Ok(snapshot(Some(FanMode::Max))), Ok(snapshot(Some(FanMode::Auto)))].into(), max: Some(Ok(applied())), restore_result: Some(Ok(applied())), calls: vec![] } }
    }
    impl FanDevice for Fake {
        fn capabilities(&mut self) -> Result<Capabilities, DeviceError> { self.calls.push("capabilities"); Ok(Capabilities { model: self.model.clone(), bios: "V1.18".into(), matched_model: true, items: vec![] }) }
        fn snapshot(&mut self) -> Result<Snapshot, DeviceError> { self.calls.push("snapshot"); self.reads.pop_front().expect("unexpected extra read") }
        fn set_max(&mut self) -> Result<ApplyReport, DeviceError> { self.calls.push("max"); self.max.take().unwrap() }
        fn restore(&mut self) -> Result<ApplyReport, DeviceError> { self.calls.push("restore"); self.restore_result.take().unwrap() }
        fn settle(&mut self) { self.calls.push("settle"); }
    }
    #[test] fn success_records_max_and_restoration_in_order() { let mut fake = Fake::normal(); let report = validate(&mut fake); assert!(report.success()); assert_eq!(fake.calls, ["capabilities", "snapshot", "max", "settle", "snapshot", "restore", "snapshot"]); assert!(report.max_readback_confirmed && report.restore_readback_confirmed); }
    #[test] fn other_model_never_writes() { let mut fake = Fake::normal(); fake.model = "PH16-71".into(); let report = validate(&mut fake); assert_eq!(report.outcome, Outcome::Rejected); assert_eq!(fake.calls, ["capabilities"]); }
    #[test] fn custom_unknown_or_failed_initial_read_never_writes() { for read in [Ok(snapshot(None)),Ok(snapshot(Some(FanMode::Custom))), Err(failure())] { let mut fake = Fake::normal(); fake.reads = [read].into(); let report = validate(&mut fake); assert_eq!(report.outcome, Outcome::Rejected); assert!(!report.max_call_attempted && !report.restore_call_attempted); } }
    #[test] fn max_error_still_attempts_restore() { let mut fake = Fake::normal(); fake.max = Some(Err(failure())); fake.reads.remove(1); let report = validate(&mut fake); assert_eq!(report.outcome, Outcome::Failed); assert!(report.restore_readback_confirmed); assert_eq!(fake.calls, ["capabilities", "snapshot", "max", "restore", "snapshot"]); }
    #[test] fn settled_read_error_still_attempts_restore() { let mut fake = Fake::normal(); fake.reads[1] = Err(failure()); let report = validate(&mut fake); assert_eq!(report.outcome, Outcome::Failed); assert!(report.restore_readback_confirmed); assert!(fake.calls.contains(&"restore")); }
    #[test] fn restore_error_is_explicit_even_if_mode_happens_to_match() { let mut fake = Fake::normal(); fake.restore_result = Some(Err(failure())); let report = validate(&mut fake); assert_eq!(report.outcome, Outcome::RestoreUnconfirmed); assert!(!report.success()); assert!(report.restore_call_attempted); }
    #[test] fn restore_readback_mismatch_is_explicit() { let mut fake = Fake::normal(); fake.reads[2] = Ok(snapshot(Some(FanMode::Max))); let report = validate(&mut fake); assert_eq!(report.outcome, Outcome::RestoreUnconfirmed); assert!(!report.restore_readback_confirmed); }
    #[test] fn original_max_is_restored_to_max_not_auto() {let mut fake=Fake::normal();fake.reads[0]=Ok(snapshot(Some(FanMode::Max)));fake.reads[2]=Ok(snapshot(Some(FanMode::Max)));fake.max.as_mut().unwrap().as_mut().unwrap().before=Some(snapshot(Some(FanMode::Max)));let report=validate(&mut fake);assert!(report.success());assert_eq!(report.original_mode,Some(FanMode::Max));}
    #[test] fn missing_initial_rpm_never_writes(){let mut fake=Fake::normal();fake.reads[0].as_mut().unwrap().gpu_fan_rpm=None;let report=validate(&mut fake);assert_eq!(report.outcome,Outcome::Rejected);assert!(!fake.calls.contains(&"max"));}
    #[test] fn failed_restore_read_remains_unconfirmed(){let mut fake=Fake::normal();fake.reads[2]=Err(failure());let report=validate(&mut fake);assert_eq!(report.outcome,Outcome::RestoreUnconfirmed);assert!(report.errors.iter().any(|e|e.phase=="restored_read"));}
}
