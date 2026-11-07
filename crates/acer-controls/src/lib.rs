//! Target-specific USB power-off charging. No automatic elevation or vendor-cache writes.
pub mod direct_settings;
use acer_protocol::{ApplyStatus,DeviceError,ErrorKind,RawRead,RawValue};
use serde::{Deserialize,Serialize};
use std::{collections::BTreeMap,sync::mpsc};

fn error(kind:ErrorKind,message:impl ToString)->DeviceError {DeviceError{kind,message:message.to_string()}}
pub fn matches_target(model:&str,bios:&str)->bool {matches!(model,"Predator PHN16-71"|"PHN16-71")&&bios=="V1.18"}
#[derive(Debug,Clone,Copy,PartialEq,Eq,Serialize,Deserialize)]
#[serde(try_from="u8",into="u8")]
pub enum UsbThreshold {Percent10,Percent20,Percent30}
impl From<UsbThreshold> for u8 {fn from(v:UsbThreshold)->u8 {match v {UsbThreshold::Percent10=>10,UsbThreshold::Percent20=>20,UsbThreshold::Percent30=>30}}}
impl TryFrom<u8> for UsbThreshold {
    type Error=DeviceError;
    fn try_from(value:u8)->Result<Self,Self::Error> {match value {10=>Ok(Self::Percent10),20=>Ok(Self::Percent20),30=>Ok(Self::Percent30),_=>Err(error(ErrorKind::InvalidArgument,"USB threshold must be 10, 20 or 30"))}}
}
#[derive(Debug,Clone,Copy,PartialEq,Eq,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UsbState {pub enabled:bool,pub threshold:UsbThreshold}
pub fn encode_usb(state:UsbState)->u64 {4|((if state.enabled {0x0fu64}else{0x1f})<<8)|((u8::from(state.threshold)as u64)<<16)}
pub fn decode_usb(output:u64)->Option<UsbState> {
    if output&255!=0 || output>>24!=0 {return None}
    let enabled=match (output>>8)&255 {0x0f=>true,0x1f=>false,_=>return None};
    let threshold=UsbThreshold::try_from(((output>>16)&255)as u8).ok()?;
    Some(UsbState{enabled,threshold})
}
fn legal_set_input(raw:&RawRead)->bool {
    if raw.input.len()!=1 {return false}
    let Some(input)=raw.input.get("uiInput").and_then(RawValue::as_u64)else{return false};
    decode_usb(input&!255).is_some_and(|state|encode_usb(state)==input)
}
#[derive(Debug,Clone,Copy,PartialEq,Eq,Serialize)]
#[serde(rename_all="snake_case")]
pub enum Ack {Confirmed,Rejected,Unknown}
fn ack(raw:&RawRead)->Ack {
    if raw.error.is_some() {return Ack::Rejected}
    if raw.output.get("ReturnValue").is_some_and(|v|v!=&RawValue::Bool(true)) {return Ack::Rejected}
    let Some(value)=raw.output.get("uiOutput").and_then(RawValue::as_u64) else{return Ack::Unknown};
    if value&255!=0 {return Ack::Rejected}
    if raw.class=="APGeAction" && raw.method=="SetFunction" && value!=0 {return Ack::Rejected}
    match raw.output.get("ReturnValue") {
        Some(RawValue::Bool(true))=>Ack::Confirmed,
        // Exact local read schema: evidence/usb-charge-read.json, 2026-11-01;
        // official CallBIOSGetFunction also consumes uiOutput rather than ReturnValue.
        // No missing-return compatibility for setters or other classes/selectors.
        None if raw.class=="APGeAction" && raw.method=="GetFunction"
            && raw.input.len()==1 && raw.input.get("uiInput").and_then(RawValue::as_u64)==Some(4)
            && raw.output.len()==1 && decode_usb(value).is_some()=>Ack::Confirmed,
        // Exact local switch/restore SET fixture: uiOutput is unsigned zero,
        // no ReturnValue. Still require a legal fixed packed request and readback.
        None if raw.class=="APGeAction" && raw.method=="SetFunction"
            && legal_set_input(raw) && raw.output.len()==1 && value==0=>Ack::Confirmed,
        _=>Ack::Unknown,
    }
}
#[derive(Debug,Clone,Serialize)]
pub struct UsbRead {
    pub state:Option<UsbState>,
    /// Structurally decoded bytes only; cannot authorize a SET when ack is unknown.
    pub decoded_state:Option<UsbState>,
    pub ack:Ack,pub raw:RawRead,
}
impl UsbRead {
    fn from_raw(raw:RawRead)->Self {
        let ack=ack(&raw);
        let decoded_state=raw.output.get("uiOutput").and_then(RawValue::as_u64).and_then(decode_usb);
        let state=if ack==Ack::Confirmed {decoded_state}else{None};
        Self{state,decoded_state,ack,raw}
    }
}
#[derive(Debug,Clone,Serialize)]
pub struct WriteAttempt {pub attempted:bool,pub raw:RawRead}
#[derive(Debug,Clone,Serialize)]
pub struct UsbApplyReport {
    pub status:ApplyStatus,pub message:String,pub target:UsbState,
    pub before:Option<UsbRead>,pub writes:Vec<WriteAttempt>,pub after:Option<UsbRead>,
}
impl UsbApplyReport {
    pub fn write_attempted(&self)->bool {self.writes.iter().any(|w|w.attempted)}
}
#[derive(Debug,Clone,Serialize)]
pub struct UsbValidationReport {
    pub original:UsbRead,pub trial:Option<UsbApplyReport>,pub restore:Option<UsbApplyReport>,
    pub restored:bool,pub restore_required:bool,pub message:String,
}
#[derive(Debug,Clone,Copy,Default)]
pub enum WritePolicy {#[default] ReadOnly,Verified,SupervisedValidation}
trait Transport {
    fn read(&mut self)->RawRead;
    fn write(&mut self,state:UsbState)->WriteAttempt;
}
struct Controller<T> {transport:T,policy:WritePolicy}
fn rejected(target:UsbState,before:Option<UsbRead>,message:&str)->UsbApplyReport {
    UsbApplyReport{status:ApplyStatus::Rejected,message:message.into(),target,before,writes:vec![],after:None}
}
impl<T:Transport> Controller<T> {
    fn get(&mut self)->UsbRead {UsbRead::from_raw(self.transport.read())}
    fn write_target(&mut self,target:UsbState,before:Option<UsbRead>)->UsbApplyReport {
        let write=self.transport.write(target);
        let after=if write.attempted {Some(self.get())}else{None};
        let status=if ack(&write.raw)==Ack::Rejected {ApplyStatus::Failed}
            else if write.attempted && ack(&write.raw)==Ack::Confirmed && after.as_ref().is_some_and(|r|r.ack==Ack::Confirmed&&r.state==Some(target)) {ApplyStatus::Applied}
            else {ApplyStatus::Unconfirmed};
        UsbApplyReport{status,message:match status {ApplyStatus::Applied=>"USB setter ACK and independent pair readback confirmed",ApplyStatus::Failed=>"USB setter failed; inspect raw error/ReturnValue/uiOutput",_=>"USB write/readback is unconfirmed; no implicit ReturnValue success"}.into(),target,before,writes:vec![write],after}
    }
    fn set(&mut self,target:UsbState)->UsbApplyReport {
        if matches!(self.policy,WritePolicy::ReadOnly){return rejected(target,None,"Read-only USB policy");}
        let before=self.get();
        if before.state.is_none(){return rejected(target,Some(before),"Original USB pair is unknown; no SET attempted");}
        self.write_target(target,Some(before))
    }
    fn validate(&mut self)->UsbValidationReport {
        self.validate_for(None)
    }
    fn validate_threshold(&mut self,threshold:UsbThreshold)->UsbValidationReport {
        self.validate_for(Some(threshold))
    }
    fn validate_for(&mut self,threshold:Option<UsbThreshold>)->UsbValidationReport {
        let original=self.get();
        let mut result=UsbValidationReport{original:original.clone(),trial:None,restore:None,restored:false,restore_required:false,message:String::new()};
        if matches!(self.policy,WritePolicy::ReadOnly){result.message="Validation requires native SupervisedValidation policy".into();return result}
        let Some(saved)=original.state else{result.message="Original USB pair/ACK unknown; no SET attempted".into();return result};
        let target=match threshold {None=>UsbState{enabled:!saved.enabled,..saved},Some(threshold)=>UsbState{threshold,..saved}};
        if target==saved {result.message="Requested threshold already matches original; no validation SET dispatched".into();return result}
        let trial=self.write_target(target,Some(original));
        let attempted=trial.write_attempted();result.trial=Some(trial);
        if attempted {
            // Saved immutable pair: do not require another successful pre-read to restore.
            let restore=self.write_target(saved,None);
            result.restored=restore.status==ApplyStatus::Applied;
            result.restore_required=!result.restored;
            result.restore=Some(restore);
        }
        result.message=if attempted && result.restored {"Validation attempt completed; original USB pair independently confirmed"}else if attempted {"Original USB pair restoration is unconfirmed; keep this report for explicit recovery"}else{"No hardware SET was dispatched"}.into();
        result
    }
}
#[derive(Clone)] pub struct UsbDevice {sender:mpsc::Sender<Request>}
enum Request {Get(mpsc::Sender<UsbRead>),Set(UsbState,mpsc::Sender<UsbApplyReport>),Validate(mpsc::Sender<UsbValidationReport>),ValidateThreshold(UsbThreshold,mpsc::Sender<UsbValidationReport>)}
fn stopped()->DeviceError {error(ErrorKind::WorkerStopped,"USB COM worker stopped")}
impl UsbDevice {
    pub fn connect()->Result<Self,DeviceError> {Self::connect_with_policy(WritePolicy::ReadOnly)}
    pub fn connect_with_policy(policy:WritePolicy)->Result<Self,DeviceError> {
        let(sender,rx)=mpsc::channel();let(ready_tx,ready_rx)=mpsc::channel();
        std::thread::Builder::new().name("acer-usb-wmi".into()).spawn(move||{
            let transport=match platform::Backend::connect(){Ok(v)=>v,Err(e)=>{let _=ready_tx.send(Err(e));return}};
            let mut controller=Controller{transport,policy};if ready_tx.send(Ok(())).is_err(){return}
            while let Ok(request)=rx.recv(){match request{
                Request::Get(tx)=>{let _=tx.send(controller.get());},Request::Set(target,tx)=>{let _=tx.send(controller.set(target));},Request::Validate(tx)=>{let _=tx.send(controller.validate());},Request::ValidateThreshold(threshold,tx)=>{let _=tx.send(controller.validate_threshold(threshold));}
            }}
        }).map_err(|e|error(ErrorKind::Transport,e))?;
        ready_rx.recv().map_err(|_|stopped())??;Ok(Self{sender})
    }
    pub fn get(&self)->Result<UsbRead,DeviceError>{let(tx,rx)=mpsc::channel();self.sender.send(Request::Get(tx)).map_err(|_|stopped())?;rx.recv().map_err(|_|stopped())}
    pub fn set(&self,target:UsbState)->Result<UsbApplyReport,DeviceError>{let(tx,rx)=mpsc::channel();self.sender.send(Request::Set(target,tx)).map_err(|_|stopped())?;rx.recv().map_err(|_|stopped())}
    pub fn validate_switch_restore(&self)->Result<UsbValidationReport,DeviceError>{let(tx,rx)=mpsc::channel();self.sender.send(Request::Validate(tx)).map_err(|_|stopped())?;rx.recv().map_err(|_|stopped())}
    pub fn validate_threshold_restore(&self,threshold:UsbThreshold)->Result<UsbValidationReport,DeviceError>{let(tx,rx)=mpsc::channel();self.sender.send(Request::ValidateThreshold(threshold,tx)).map_err(|_|stopped())?;rx.recv().map_err(|_|stopped())}
}
fn raw_read(method:&str,input:u64,output:BTreeMap<String,RawValue>,err:Option<DeviceError>)->RawRead {
    RawRead{class:"APGeAction".into(),method:method.into(),input:BTreeMap::from([("uiInput".into(),RawValue::unsigned(input))]),output,error:err}
}
#[cfg(windows)] mod platform;
#[cfg(test)] mod tests;
#[cfg(not(windows))] mod platform {
    use super::*;
    pub(super) struct Backend;
    impl Backend {pub fn connect()->Result<Self,DeviceError>{Err(error(ErrorKind::Unsupported,"USB WMI requires Windows"))}}
    impl Transport for Backend {
        fn read(&mut self)->RawRead{raw_read("GetFunction",4,BTreeMap::new(),Some(stopped()))}
        fn write(&mut self,state:UsbState)->WriteAttempt{WriteAttempt{attempted:false,raw:raw_read("SetFunction",encode_usb(state),BTreeMap::new(),Some(stopped()))}}
    }
}
