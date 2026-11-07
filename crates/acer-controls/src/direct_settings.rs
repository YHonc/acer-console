//! Fixed, target-specific direct-WMI settings with reviewed ACK schemas.
use acer_protocol::{ApplyStatus, DeviceError, ErrorKind, RawRead, RawValue};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::mpsc};
pub use crate::{Ack, WriteAttempt};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Setting { LcdOverdrive, BootSound, BacklightTimeout }
impl Setting {
    pub const ALL: [Self; 3] = [Self::LcdOverdrive, Self::BootSound, Self::BacklightTimeout];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "setting", rename_all = "snake_case")]
pub enum State {
    LcdOverdrive { enabled: bool },
    BootSound { enabled: bool },
    BacklightTimeout { enabled: bool, brightness: u8 },
}
impl State {
    pub fn enabled(self) -> bool { match self {
        Self::LcdOverdrive { enabled } | Self::BootSound { enabled } |
        Self::BacklightTimeout { enabled, .. } => enabled,
    } }
    pub fn setting(self) -> Setting { match self {
        Self::LcdOverdrive { .. } => Setting::LcdOverdrive,
        Self::BootSound { .. } => Setting::BootSound,
        Self::BacklightTimeout { .. } => Setting::BacklightTimeout,
    } }
    fn with_enabled(self, enabled: bool) -> Self { match self {
        Self::LcdOverdrive { .. } => Self::LcdOverdrive { enabled },
        Self::BootSound { .. } => Self::BootSound { enabled },
        Self::BacklightTimeout { brightness, .. } => Self::BacklightTimeout { enabled, brightness },
    } }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum WritePolicy { #[default] ReadOnly, Verified, SupervisedValidation }
#[derive(Debug, Clone, Serialize)]
pub struct Identity { pub model: String, pub bios: String }
#[derive(Debug, Clone, Serialize)]
pub struct SettingRead {
    pub setting: Setting,
    pub state: Option<State>,
    /// Structural decoding only. Never authorizes a write without a confirmed GET ACK.
    pub decoded_state: Option<State>,
    pub ack: Ack,
    pub raw: RawRead,
}
#[derive(Debug, Clone, Serialize)]
pub struct ReadReport { pub identity: Identity, pub readings: Vec<SettingRead> }
#[derive(Debug, Clone, Serialize)]
pub struct ApplyReport {
    pub setting: Setting, pub enabled: bool, pub status: ApplyStatus, pub message: String,
    pub before: Option<SettingRead>, pub expected: Option<State>,
    pub writes: Vec<WriteAttempt>, pub write_ack: Option<Ack>,
    pub after: Option<SettingRead>, pub readback_confirmed: bool,
}
impl ApplyReport {
    pub fn write_attempted(&self) -> bool { self.writes.iter().any(|w| w.attempted) }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidationOutcome { Rejected, Confirmed, ReadbacksConfirmedAcksUnknown, TrialUnconfirmed, RestoreUnconfirmed }
#[derive(Debug, Clone, Serialize)]
pub struct ValidationReport {
    pub identity: Identity, pub setting: Setting, pub outcome: ValidationOutcome,
    pub original: SettingRead, pub trial: Option<ApplyReport>, pub restore: Option<ApplyReport>,
    pub target_readback_confirmed: bool, pub restore_readback_confirmed: bool,
    pub original_state_restored: bool, pub setter_acks_confirmed: bool,
    pub restore_required: bool, pub message: String,
}
impl ValidationReport { pub fn success(&self) -> bool { self.outcome == ValidationOutcome::Confirmed } }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Call { class: &'static str, method: &'static str, parameter: &'static str, value: u64 }
impl Call {
    fn raw(self, output: BTreeMap<String, RawValue>, error: Option<DeviceError>) -> RawRead {
        RawRead { class:self.class.into(), method:self.method.into(),
            input:BTreeMap::from([(self.parameter.into(), RawValue::unsigned(self.value))]), output, error }
    }
}
fn get_call(setting: Setting) -> Call { match setting {
    Setting::LcdOverdrive => Call { class:"AcerGamingFunction", method:"GetGamingProfile", parameter:"gmInput", value:0 },
    Setting::BootSound => Call { class:"AcerGamingFunction", method:"GetGamingMiscSetting", parameter:"gmInput", value:6 },
    Setting::BacklightTimeout => Call { class:"APGeAction", method:"GetFunction", parameter:"uiInput", value:0x88401 },
} }
fn set_call(target: State) -> Call { match target {
    State::LcdOverdrive { enabled } => Call { class:"AcerGamingFunction", method:"SetGamingProfile", parameter:"gmInput", value:0x10 | (u64::from(enabled) << 48) },
    State::BootSound { enabled } => Call { class:"AcerGamingFunction", method:"SetGamingMiscSetting", parameter:"gmInput", value:6 | (u64::from(enabled) << 8) },
    State::BacklightTimeout { enabled, brightness } => Call { class:"APGeAction", method:"SetFunction", parameter:"uiInput",
        value:0x88402 | (u64::from(brightness) << 32) | (if enabled { 30u64 << 40 } else { 0 }) },
} }
fn output_field(call: Call) -> &'static str { if call.class == "APGeAction" { "uiOutput" } else { "gmOutput" } }
fn matches_call(call: Call, raw: &RawRead) -> bool {
    raw.class == call.class && raw.method == call.method && raw.input.len() == 1 &&
        raw.input.get(call.parameter) == Some(&RawValue::unsigned(call.value))
}
fn legal_set_call(call: Call) -> bool {
    match (call.class,call.method,call.parameter) {
        ("AcerGamingFunction","SetGamingProfile","gmInput") =>
            [false,true].into_iter().any(|enabled|set_call(State::LcdOverdrive{enabled})==call),
        ("AcerGamingFunction","SetGamingMiscSetting","gmInput") =>
            [false,true].into_iter().any(|enabled|set_call(State::BootSound{enabled})==call),
        ("APGeAction","SetFunction","uiInput") => {
            let brightness=((call.value>>32)&255) as u8;
            brightness<=100 && [false,true].into_iter().any(|enabled|
                set_call(State::BacklightTimeout{enabled,brightness})==call)
        }
        _ => false,
    }
}
fn acknowledge(call: Call, raw: &RawRead, write: bool) -> Ack {
    if !matches_call(call, raw) || raw.error.is_some() { return Ack::Rejected; }
    if raw.output.get("ReturnValue").is_some_and(|v| v != &RawValue::Bool(true)) { return Ack::Rejected; }
    let Some(value) = raw.output.get(output_field(call)).and_then(RawValue::as_u64) else { return Ack::Unknown; };
    if (write && value != 0) || (!write && value & 255 != 0) { return Ack::Rejected; }
    // Exact local GET-only schemas: evidence/direct-settings-read.json. Each
    // fixed request returns just its unsigned output field, with no ReturnValue.
    if !write && !raw.output.contains_key("ReturnValue") && raw.output.len()==1 &&
        Setting::ALL.into_iter().any(|setting| get_call(setting)==call && decode(setting,value).is_some()) {
        return Ack::Confirmed;
    }
    // Exact local trial/restore schemas: evidence/direct-{lcd_overdrive,
    // boot_sound,backlight_timeout}-restore.json. No compatibility for other
    // selectors, extra input bits/properties, extra outputs or non-UInt zero.
    if write && !raw.output.contains_key("ReturnValue") && raw.output.len()==1 &&
        value==0 && legal_set_call(call) { return Ack::Confirmed; }
    if raw.output.get("ReturnValue") == Some(&RawValue::Bool(true)) { Ack::Confirmed } else { Ack::Unknown }
}
fn decode(setting: Setting, value: u64) -> Option<State> {
    if value & 255 != 0 { return None; }
    if setting==Setting::BacklightTimeout && ((value>>32)&255)>100 { return None; }
    let flag = match setting { Setting::LcdOverdrive => (value >> 48) & 255,
        Setting::BootSound => (value >> 8) & 255, Setting::BacklightTimeout => (value >> 40) & 255 };
    let enabled = match (setting, flag) {
        (Setting::BacklightTimeout, 0) => false, (Setting::BacklightTimeout, 30) => true,
        (Setting::LcdOverdrive | Setting::BootSound, 0) => false,
        (Setting::LcdOverdrive | Setting::BootSound, 1) => true, _ => return None,
    };
    Some(match setting { Setting::LcdOverdrive => State::LcdOverdrive { enabled },
        Setting::BootSound => State::BootSound { enabled },
        Setting::BacklightTimeout => State::BacklightTimeout { enabled, brightness:((value >> 32) & 255) as u8 } })
}
fn read_result(setting: Setting, raw: RawRead) -> SettingRead {
    let call=get_call(setting); let ack=acknowledge(call, &raw, false);
    let decoded_state=raw.output.get(output_field(call)).and_then(RawValue::as_u64).and_then(|v|decode(setting,v));
    let state=if ack == Ack::Confirmed { decoded_state } else { None };
    SettingRead { setting, state, decoded_state, ack, raw }
}
trait Transport { fn call(&mut self, call: Call, write: bool) -> WriteAttempt; }
struct Controller<T> { transport:T, policy:WritePolicy, identity:Identity }
fn error(kind:ErrorKind, message:impl ToString) -> DeviceError { DeviceError { kind, message:message.to_string() } }
fn rejected(setting:Setting, enabled:bool, before:Option<SettingRead>, message:&str) -> ApplyReport {
    ApplyReport { setting, enabled, status:ApplyStatus::Rejected, message:message.into(), before,
        expected:None, writes:vec![], write_ack:None, after:None, readback_confirmed:false }
}
impl<T:Transport> Controller<T> {
    fn new(transport:T, policy:WritePolicy, identity:Identity) -> Result<Self,DeviceError> {
        if !crate::matches_target(&identity.model,&identity.bios) {
            return Err(error(ErrorKind::Unsupported,"Direct settings require PHN16-71 / BIOS V1.18"));
        }
        Ok(Self { transport, policy, identity })
    }
    fn get(&mut self, setting:Setting) -> SettingRead { read_result(setting,self.transport.call(get_call(setting),false).raw) }
    fn read_all(&mut self) -> ReadReport { ReadReport { identity:self.identity.clone(), readings:Setting::ALL.into_iter().map(|s|self.get(s)).collect() } }
    fn write_known(&mut self, target:State, before:Option<SettingRead>) -> ApplyReport {
        if matches!(target,State::BacklightTimeout{brightness,..} if brightness>100) {
            return rejected(target.setting(),target.enabled(),before,"Backlight brightness must be confirmed within 0..100; no SET dispatched");
        }
        let call=set_call(target); let write=self.transport.call(call,true);
        let ack=acknowledge(call,&write.raw,true);
        let after=if write.attempted { Some(self.get(target.setting())) } else { None };
        let readback_confirmed=after.as_ref().is_some_and(|r|r.ack==Ack::Confirmed && r.state==Some(target));
        let status=if ack==Ack::Rejected { ApplyStatus::Failed }
            else if write.attempted && ack==Ack::Confirmed && readback_confirmed { ApplyStatus::Applied }
            else { ApplyStatus::Unconfirmed };
        ApplyReport { setting:target.setting(), enabled:target.enabled(), status,
            message:match status { ApplyStatus::Applied=>"Reviewed fixed setter ACK and independent exact-state GET confirmed",
                ApplyStatus::Failed=>"Direct setter failed; inspect raw method/firmware error",
                _=>"Direct SET ACK/readback remains unconfirmed; readback never substitutes for ACK" }.into(),
            before, expected:Some(target), writes:vec![write], write_ack:Some(ack), after, readback_confirmed }
    }
    fn set(&mut self, setting:Setting, enabled:bool) -> ApplyReport {
        if self.policy==WritePolicy::ReadOnly { return rejected(setting,enabled,None,"Read-only policy; native Verified or SupervisedValidation required"); }
        let before=self.get(setting);
        let Some(state)=before.state else { return rejected(setting,enabled,Some(before),"Fresh GET state/ACK unknown; no SET dispatched"); };
        self.write_known(state.with_enabled(enabled),Some(before))
    }
    fn restore(&mut self, saved:State) -> ApplyReport {
        if saved.setting()==Setting::BacklightTimeout {
            // Preserve the brightness from a fresh, acknowledged GET for this SET.
            // An immutable old brightness must not overwrite a concurrent change.
            self.set(Setting::BacklightTimeout,saved.enabled())
        } else { self.write_known(saved,None) }
    }
    fn validate(&mut self, setting:Setting) -> ValidationReport {
        let original=self.get(setting);
        let mut report=ValidationReport { identity:self.identity.clone(), setting, outcome:ValidationOutcome::Rejected,
            original:original.clone(), trial:None, restore:None, target_readback_confirmed:false,
            restore_readback_confirmed:false, original_state_restored:false, setter_acks_confirmed:false,
            restore_required:false, message:String::new() };
        if self.policy!=WritePolicy::SupervisedValidation { report.message="Validation requires explicit native SupervisedValidation policy; no SET dispatched".into(); return report; }
        let Some(saved)=original.state else { report.message="Original GET state/ACK unknown; no SET dispatched".into(); return report; };
        let trial=self.write_known(saved.with_enabled(!saved.enabled()),Some(original));
        let attempted=trial.write_attempted(); report.target_readback_confirmed=trial.readback_confirmed;
        report.trial=Some(trial);
        if !attempted { report.message="Trial SET was not dispatched; no restore required".into(); return report; }
        // Even a transport error/unknown ACK may have reached firmware. Never
        // return after an attempted trial without attempting original recovery.
        let restore=self.restore(saved);
        report.restore_readback_confirmed=restore.readback_confirmed;
        report.original_state_restored=restore.after.as_ref().is_some_and(|r|r.ack==Ack::Confirmed && r.state==Some(saved));
        let trial=report.trial.as_ref().unwrap();
        report.setter_acks_confirmed=trial.write_ack==Some(Ack::Confirmed) && restore.write_ack==Some(Ack::Confirmed);
        report.restore_required=restore.status!=ApplyStatus::Applied || !report.original_state_restored;
        report.outcome=if !report.restore_readback_confirmed || !report.original_state_restored || restore.write_ack==Some(Ack::Rejected) { ValidationOutcome::RestoreUnconfirmed }
            else if trial.status==ApplyStatus::Applied && restore.status==ApplyStatus::Applied { ValidationOutcome::Confirmed }
            else if report.target_readback_confirmed && trial.write_ack!=Some(Ack::Rejected)
                && (trial.write_ack==Some(Ack::Unknown) || restore.write_ack==Some(Ack::Unknown)) { ValidationOutcome::ReadbacksConfirmedAcksUnknown }
            else { ValidationOutcome::TrialUnconfirmed };
        report.restore=Some(restore);
        report.message=if report.success() { "Trial and original state restored with independent GETs and reviewed fixed setter ACKs" }
            else if report.outcome==ValidationOutcome::ReadbacksConfirmedAcksUnknown { "Both readbacks match, but setter ACK remains unknown; no Applied claim or missing-return compatibility" }
            else if report.restore_required { "Original restoration/ACK remains unconfirmed; retain this report for supervised recovery" }
            else { "Original restore confirmed; trial or its setter ACK remains unconfirmed" }.into();
        report
    }
}

#[derive(Clone)]
pub struct DirectSettingsDevice { sender:mpsc::Sender<Request> }
enum Request {
    Get(Setting,mpsc::Sender<SettingRead>), ReadAll(mpsc::Sender<ReadReport>),
    Set(Setting,bool,mpsc::Sender<ApplyReport>), Validate(Setting,mpsc::Sender<ValidationReport>),
}
fn stopped() -> DeviceError { error(ErrorKind::WorkerStopped,"Direct settings COM worker stopped") }
impl DirectSettingsDevice {
    pub fn connect() -> Result<Self,DeviceError> { Self::connect_with_policy(WritePolicy::ReadOnly) }
    pub fn connect_with_policy(policy:WritePolicy) -> Result<Self,DeviceError> {
        let(sender,rx)=mpsc::channel(); let(ready_tx,ready_rx)=mpsc::channel();
        std::thread::Builder::new().name("acer-direct-settings-wmi".into()).spawn(move || {
            let mut controller=match platform::Backend::connect().and_then(|(backend,identity)|Controller::new(backend,policy,identity)) {
                Ok(c)=>c, Err(e)=>{let _=ready_tx.send(Err(e));return;}
            };
            if ready_tx.send(Ok(())).is_err() { return; }
            while let Ok(request)=rx.recv() { match request {
                Request::Get(setting,tx)=>{let _=tx.send(controller.get(setting));},
                Request::ReadAll(tx)=>{let _=tx.send(controller.read_all());},
                Request::Set(setting,enabled,tx)=>{let _=tx.send(controller.set(setting,enabled));},
                Request::Validate(setting,tx)=>{let _=tx.send(controller.validate(setting));},
            } }
        }).map_err(|e|error(ErrorKind::Transport,e))?;
        ready_rx.recv().map_err(|_|stopped())??; Ok(Self { sender })
    }
    pub fn get(&self, setting:Setting) -> Result<SettingRead,DeviceError> { let(tx,rx)=mpsc::channel();self.sender.send(Request::Get(setting,tx)).map_err(|_|stopped())?;rx.recv().map_err(|_|stopped()) }
    pub fn read_all(&self) -> Result<ReadReport,DeviceError> { let(tx,rx)=mpsc::channel();self.sender.send(Request::ReadAll(tx)).map_err(|_|stopped())?;rx.recv().map_err(|_|stopped()) }
    pub fn set(&self, setting:Setting, enabled:bool) -> Result<ApplyReport,DeviceError> { let(tx,rx)=mpsc::channel();self.sender.send(Request::Set(setting,enabled,tx)).map_err(|_|stopped())?;rx.recv().map_err(|_|stopped()) }
    pub fn validate_restore(&self, setting:Setting) -> Result<ValidationReport,DeviceError> { let(tx,rx)=mpsc::channel();self.sender.send(Request::Validate(setting,tx)).map_err(|_|stopped())?;rx.recv().map_err(|_|stopped()) }
}
#[cfg(windows)] #[path="direct_settings/platform.rs"] mod platform;
#[cfg(test)] #[path="direct_settings/tests.rs"] mod tests;
#[cfg(not(windows))] mod platform {
    use super::*;
    pub(super) struct Backend;
    impl Backend { pub fn connect() -> Result<(Self,Identity),DeviceError> { Err(error(ErrorKind::Unsupported,"Direct settings require Windows WMI")) } }
    impl Transport for Backend { fn call(&mut self,call:Call,_:bool) -> WriteAttempt { WriteAttempt { attempted:false,raw:call.raw(BTreeMap::new(),Some(stopped())) } } }
}
