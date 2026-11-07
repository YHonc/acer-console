//! Fixed PHN16-71 audio controls through the installed official Agent.
//! No RESense/Nitro pipe, arbitrary endpoint, function name or request JSON API.
use acer_protocol::{ApplyStatus, Capabilities, DeviceError, ErrorKind};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{io::{Read, Write}, net::{SocketAddr, TcpStream}, sync::Mutex,
    time::{Duration, Instant}};

const MAX_RESPONSE:usize=64*1024;
// A production SET is bounded by 7.8s of network/settle work; the two-SET
// validation including recovery is bounded by 14.1s, below the helper's 15s.
const READ_TIMEOUT:Duration=Duration::from_millis(750);
const SET_TIMEOUT:Duration=Duration::from_secs(3);
const SETTLE_INTERVAL:Duration=Duration::from_millis(100);
const MAX_READBACK_ATTEMPTS:usize=4;
// Exact SET100 schema observed in evidence/audio-music-first-restore.json.
const SETTER_ACK_SCHEMA_VERIFIED:bool=true;
// Serialize the complete preflight/apply/readback or validation/restore sequence
// across native clients in this process, not just each individual TCP request.
static AUDIO_OPERATION_LOCK:Mutex<()>=Mutex::new(());

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all="snake_case")]
pub enum AudioPreset { Music, Movie, Voice, Strategy, Rpg, Shooter, Custom, Automatic }
impl AudioPreset {
    pub const ALL:[Self;8]=[Self::Music,Self::Movie,Self::Voice,Self::Strategy,
        Self::Rpg,Self::Shooter,Self::Custom,Self::Automatic];
    pub fn code(self)->u64 { match self { Self::Music=>0,Self::Movie=>1,Self::Voice=>2,
        Self::Strategy=>3,Self::Rpg=>4,Self::Shooter=>5,Self::Custom=>6,Self::Automatic=>10 } }
    fn from_code(code:u64)->Option<Self> { Self::ALL.into_iter().find(|p|p.code()==code) }
    fn from_capability(name:&str)->Option<Self> { match name {
        "MUSIC"=>Some(Self::Music),"MOVIE"=>Some(Self::Movie),"VOICE"=>Some(Self::Voice),
        "STRATEGY"=>Some(Self::Strategy),"RPG"=>Some(Self::Rpg),"SHOOTER"=>Some(Self::Shooter),
        "CUSTOM_AUDIO"=>Some(Self::Custom),"AUTOMATIC"=>Some(Self::Automatic),_=>None,
    } }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AudioCapability { pub supported:Vec<AudioPreset>, pub unrecognized:Vec<String> }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag="availability", rename_all="snake_case")]
pub enum AudioState {
    Available { preset:AudioPreset },
    /// Official/RESense enum code9 is an unavailable output, never an Off preset.
    OutputUnavailable { raw_mode:u64 },
}
impl AudioState { pub fn preset(self)->Option<AudioPreset> { match self {
    Self::Available{preset}=>Some(preset),Self::OutputUnavailable{..}=>None,
} } }
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum WritePolicy { #[default] ReadOnly, ExplicitApply, SupervisedValidation }
#[derive(Debug, Clone, Serialize)]
pub struct AudioIdentity { pub model:String, pub bios:String }
#[derive(Debug, Clone, Serialize)]
pub struct RawExchange {
    pub packet_id:u32, pub request:Value, pub response:Option<Value>,
    pub dispatch_attempted:bool, pub bytes_written:usize, pub bytes_received:usize,
    pub error:Option<DeviceError>,
}
#[derive(Debug, Clone, Serialize)]
pub struct AudioRead<T> { pub value:Option<T>, pub raw:RawExchange, pub error:Option<DeviceError> }
#[derive(Debug, Clone, Serialize)]
pub struct AudioQuery {
    pub identity:AudioIdentity, pub capability:AudioRead<AudioCapability>,
    pub state:AudioRead<AudioState>, pub writable:bool, pub setter_ack_schema_verified:bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all="snake_case")]
pub enum AudioAck { Confirmed, Rejected, Unknown }
#[derive(Debug, Clone, Serialize)]
pub struct AudioApplyReport {
    pub target:AudioPreset, pub status:ApplyStatus, pub message:String,
    pub capability:Option<AudioRead<AudioCapability>>, pub before:Option<AudioRead<AudioState>>,
    pub write:Option<RawExchange>, pub ack:Option<AudioAck>, pub after:Option<AudioRead<AudioState>>,
    pub readback_confirmed:bool, pub readback_attempts:Vec<AudioRead<AudioState>>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all="snake_case")]
pub enum AudioValidationOutcome { Rejected, Confirmed, ReadbacksConfirmedAcksUnknown, TrialUnconfirmed, RestoreUnconfirmed }
#[derive(Debug, Clone, Serialize)]
pub struct AudioValidationReport {
    pub identity:AudioIdentity, pub target:AudioPreset, pub outcome:AudioValidationOutcome,
    pub capability:Option<AudioRead<AudioCapability>>, pub before:Option<AudioRead<AudioState>>,
    pub original:Option<AudioPreset>, pub trial:Option<AudioApplyReport>, pub restore:Option<AudioApplyReport>,
    pub restore_attempted:bool, pub target_readback_confirmed:bool, pub original_state_restored:bool,
    pub setter_acks_confirmed:bool, pub restore_required:bool, pub message:String,
}
impl AudioValidationReport { pub fn success(&self)->bool { self.outcome==AudioValidationOutcome::Confirmed } }

fn failure(kind:ErrorKind,message:impl ToString)->DeviceError { DeviceError{kind,message:message.to_string()} }
fn protocol(message:impl ToString)->DeviceError { failure(ErrorKind::Protocol,message) }
#[derive(Debug, Clone)]
struct TargetGuard { identity:AudioIdentity }
impl TargetGuard {
    fn from_capabilities(caps:&Capabilities)->Result<Self,DeviceError> {
        if !caps.matched_model || !acer_device::verified_fan_target(&caps.model,&caps.bios) {
            return Err(failure(ErrorKind::Unsupported,"Audio controls require actual PHN16-71 / BIOS V1.18 capabilities"));
        }
        Ok(Self{identity:AudioIdentity{model:caps.model.clone(),bios:caps.bios.clone()}})
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FixedRequest { Capability, State, Set(AudioPreset) }
impl FixedRequest {
    fn packet_id(self)->u32 { match self { Self::Capability=>0,Self::State=>20,Self::Set(_)=>100 } }
    fn timeout(self)->Duration { match self { Self::Set(_)=>SET_TIMEOUT,_=>READ_TIMEOUT } }
    fn function(self)->&'static str { match self { Self::Capability=>"SUPPORT_AUDIO_MODE",_=>"SOUND_MODE" } }
    fn payload(self)->Value { match self {
        Self::Set(preset)=>json!({"Function":"SOUND_MODE","Parameter":{"mode":preset.code(),"function":"DTS"}}),
        _=>json!({"Function":self.function()}),
    } }
    fn frame(self)->Result<Vec<u8>,DeviceError> {
        let mut frame=b"ACER".to_vec();frame.extend_from_slice(&self.packet_id().to_le_bytes());
        frame.extend(serde_json::to_vec(&self.payload()).map_err(protocol)?);Ok(frame)
    }
    fn empty_exchange(self)->RawExchange { RawExchange { packet_id:self.packet_id(),request:self.payload(),
        response:None,dispatch_attempted:false,bytes_written:0,bytes_received:0,error:None } }
}
fn parse_capability(value:&Value)->Result<AudioCapability,DeviceError> {
    if value.get("request").and_then(Value::as_str)!=Some("SUPPORT_AUDIO_MODE") {
        return Err(protocol("Audio capability response request mismatch"));
    }
    // Exact observed INIT format: numeric0, not string "0" or Boolean false.
    if value.get("result").and_then(Value::as_u64)!=Some(0) {
        return Err(protocol("Audio capability result is not the observed numeric zero"));
    }
    let names=value.get("data").and_then(Value::as_array).ok_or_else(||protocol("Audio capability data must be a string array"))?;
    let mut supported=vec![];let mut unrecognized=vec![];
    for name in names {
        let name=name.as_str().ok_or_else(||protocol("Audio capability contains a non-string mode"))?;
        if let Some(preset)=AudioPreset::from_capability(name) {
            if !supported.contains(&preset) { supported.push(preset); }
        } else { unrecognized.push(name.to_string()); }
    }
    Ok(AudioCapability{supported,unrecognized})
}
fn parse_state(value:&Value)->Result<AudioState,DeviceError> {
    if value.get("request").and_then(Value::as_str)!=Some("SOUND_MODE") {
        return Err(protocol("Audio state response request mismatch"));
    }
    // Exact observed GET format: string "0", not numeric0.
    if value.get("result").and_then(Value::as_str)!=Some("0") {
        return Err(protocol("Audio state result is not the observed string zero"));
    }
    let data=value.get("data").and_then(Value::as_object).ok_or_else(||protocol("Audio state data must be an object"))?;
    if data.get("function").and_then(Value::as_str)!=Some("DTS") {
        return Err(protocol("Audio state function is not DTS"));
    }
    let code=data.get("mode").and_then(Value::as_u64).ok_or_else(||protocol("Audio state mode must be an unsigned integer"))?;
    if code==9 { return Ok(AudioState::OutputUnavailable{raw_mode:9}); }
    AudioPreset::from_code(code).map(|preset|AudioState::Available{preset})
        .ok_or_else(||protocol(format!("Unrecognized audio mode {code}; no writable original preset")))
}
fn parse_set_ack(raw:&RawExchange,target:AudioPreset)->AudioAck {
    let request=FixedRequest::Set(target);
    if raw.packet_id!=request.packet_id() || raw.request!=request.payload() { return AudioAck::Rejected; }
    if raw.error.is_some() { return AudioAck::Unknown; }
    if !raw.dispatch_attempted || request.frame().ok().is_none_or(|frame|raw.bytes_written!=frame.len()) {
        return AudioAck::Unknown;
    }
    let Some(response)=raw.response.as_ref().and_then(Value::as_object) else { return AudioAck::Unknown; };
    // Local trial and restore both return exactly these two fields. In
    // particular, numeric0 or an extra data/status field is not this schema.
    if response.len()!=2 { return AudioAck::Unknown; }
    match response.get("request").and_then(Value::as_str) {
        Some("SOUND_MODE")=>{},Some(_)=>return AudioAck::Rejected,None=>return AudioAck::Unknown,
    }
    match response.get("result") {
        Some(Value::String(result)) if result=="0"=>AudioAck::Confirmed,
        Some(Value::String(result)) if result.parse::<i64>().is_ok_and(|code|code!=0)=>AudioAck::Rejected,
        Some(Value::Number(result)) if result.as_f64().is_some_and(|code|code!=0.0)=>AudioAck::Rejected,
        _=>AudioAck::Unknown,
    }
}
fn remaining(deadline:Instant)->Result<Duration,DeviceError> {
    let remaining=deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() { Err(failure(ErrorKind::Transport,"Official audio exchange exceeded its overall request deadline; SET outcome may be unknown")) }
    else { Ok(remaining) }
}
fn complete_response(bytes:&[u8])->Result<Option<Value>,DeviceError> {
    if bytes.len()>MAX_RESPONSE { return Err(protocol("Official audio response exceeds 64KiB")); }
    match serde_json::from_slice(bytes) {
        Ok(value)=>Ok(Some(value)),Err(e) if e.is_eof()=>Ok(None),Err(e)=>Err(protocol(e)),
    }
}
trait Transport {
    fn exchange(&mut self,request:FixedRequest)->RawExchange;
    fn settle(&mut self) { std::thread::sleep(SETTLE_INTERVAL); }
}
struct Network;
impl Transport for Network {
    fn exchange(&mut self,request:FixedRequest)->RawExchange {
        let mut raw=request.empty_exchange();
        let deadline=Instant::now()+request.timeout();
        let result=(||->Result<Value,DeviceError> {
            let frame=request.frame()?;
            let address=SocketAddr::from(([127,0,0,1],46933));
            let mut stream=TcpStream::connect_timeout(&address,remaining(deadline)?)
                .map_err(|e|failure(ErrorKind::Transport,e))?;
            while raw.bytes_written<frame.len() {
                stream.set_write_timeout(Some(remaining(deadline)?)).map_err(|e|failure(ErrorKind::Transport,e))?;
                raw.dispatch_attempted=true;
                match stream.write(&frame[raw.bytes_written..]) {
                    Ok(0)=>return Err(failure(ErrorKind::Transport,"Official audio service closed during request write")),
                    Ok(count)=>raw.bytes_written+=count,
                    Err(e) if e.kind()==std::io::ErrorKind::Interrupted=>continue,
                    Err(e)=>return Err(failure(ErrorKind::Transport,e)),
                }
            }
            let mut response=Vec::new();let mut chunk=[0u8;4096];
            loop {
                stream.set_read_timeout(Some(remaining(deadline)?)).map_err(|e|failure(ErrorKind::Transport,e))?;
                let count=match stream.read(&mut chunk) {
                    Ok(0)=>return Err(failure(ErrorKind::Transport,"Official audio service closed before complete JSON")),
                    Ok(count)=>count,Err(e) if e.kind()==std::io::ErrorKind::Interrupted=>continue,
                    Err(e)=>return Err(failure(ErrorKind::Transport,e)),
                };
                remaining(deadline)?;
                raw.bytes_received+=count;response.extend_from_slice(&chunk[..count]);
                if let Some(value)=complete_response(&response)? { return Ok(value); }
            }
        })();
        match result { Ok(value)=>raw.response=Some(value),Err(e)=>raw.error=Some(e) }
        raw
    }
}
struct Controller<T> { transport:T, guard:TargetGuard, policy:WritePolicy }
fn rejected(target:AudioPreset,message:&str)->AudioApplyReport {
    AudioApplyReport { target,status:ApplyStatus::Rejected,message:message.into(),capability:None,before:None,
        write:None,ack:None,after:None,readback_confirmed:false,readback_attempts:vec![] }
}
impl<T:Transport> Controller<T> {
    fn read<U>(&mut self,request:FixedRequest,parse:fn(&Value)->Result<U,DeviceError>)->AudioRead<U> {
        let raw=self.transport.exchange(request);
        let result=if let Some(e)=raw.error.clone() { Err(e) }
            else { raw.response.as_ref().ok_or_else(||protocol("Missing official audio JSON response")).and_then(parse) };
        match result { Ok(value)=>AudioRead{value:Some(value),raw,error:None},Err(e)=>AudioRead{value:None,raw,error:Some(e)} }
    }
    fn capability(&mut self)->AudioRead<AudioCapability> { self.read(FixedRequest::Capability,parse_capability) }
    fn state(&mut self)->AudioRead<AudioState> { self.read(FixedRequest::State,parse_state) }
    fn query(&mut self)->AudioQuery {
        let capability=self.capability();let state=self.state();
        let original=state.value.and_then(AudioState::preset);
        let writable=self.policy!=WritePolicy::ReadOnly && SETTER_ACK_SCHEMA_VERIFIED
            && original.is_some_and(|preset|capability.value.as_ref().is_some_and(|c|c.supported.contains(&preset)));
        AudioQuery { identity:self.guard.identity.clone(),capability,state,writable,setter_ack_schema_verified:SETTER_ACK_SCHEMA_VERIFIED }
    }
    fn write_known(&mut self,target:AudioPreset,before:Option<AudioRead<AudioState>>)->AudioApplyReport {
        let write=self.transport.exchange(FixedRequest::Set(target));let ack=parse_set_ack(&write,target);
        // A timeout may arrive after the service changed its setting. Read even
        // after an error and retain both the error and the independent state.
        let mut readback_attempts=Vec::with_capacity(MAX_READBACK_ATTEMPTS);
        let mut readback_confirmed=false;
        for attempt in 0..MAX_READBACK_ATTEMPTS {
            if attempt>0 { self.transport.settle(); }
            let read=self.state();readback_confirmed=read.value==Some(AudioState::Available{preset:target});
            readback_attempts.push(read);
            if readback_confirmed { break; }
        }
        let after=readback_attempts.last().cloned();
        let status=if ack==AudioAck::Rejected { ApplyStatus::Failed }
            else if ack==AudioAck::Confirmed && readback_confirmed { ApplyStatus::Applied }
            else { ApplyStatus::Unconfirmed };
        AudioApplyReport { target,status,message:match status {
            ApplyStatus::Applied=>"Reviewed audio setter ACK and independent GET matched",
            ApplyStatus::Failed=>"Audio reply identity was rejected; inspect raw response and GET",
            _=>"Audio ACK or bounded GET readback remains unconfirmed; inspect every recorded attempt",
        }.into(),capability:None,before,write:Some(write),ack:Some(ack),after,readback_confirmed,readback_attempts }
    }
    fn set(&mut self,target:AudioPreset)->AudioApplyReport {
        if self.policy==WritePolicy::ReadOnly { return rejected(target,"Read-only audio policy; explicit native write authorization required"); }
        if self.policy==WritePolicy::ExplicitApply && !SETTER_ACK_SCHEMA_VERIFIED {
            return rejected(target,"Production audio apply requires a reviewed setter ACK schema");
        }
        let capability=self.capability();let before=self.state();
        let allowed=capability.value.as_ref().is_some_and(|c|c.supported.contains(&target));
        let known=before.value.and_then(AudioState::preset)
            .is_some_and(|preset|capability.value.as_ref().is_some_and(|c|c.supported.contains(&preset)));
        let mut report=if !allowed || !known {
            rejected(target,"Known current audio preset and advertised target capability required; unavailable9/unknown never writes")
        } else { self.write_known(target,Some(before.clone())) };
        report.capability=Some(capability);report.before=Some(before);report
    }
    fn validate_restore(&mut self,target:AudioPreset)->AudioValidationReport {
        let mut report=AudioValidationReport { identity:self.guard.identity.clone(),target,outcome:AudioValidationOutcome::Rejected,
            capability:None,before:None,original:None,trial:None,restore:None,restore_attempted:false,
            target_readback_confirmed:false,original_state_restored:false,setter_acks_confirmed:false,
            restore_required:false,message:String::new() };
        if self.policy!=WritePolicy::SupervisedValidation {
            report.message="Audio validation requires native SupervisedValidation policy; no requests sent".into();return report;
        }
        let capability=self.capability();let before=self.state();
        let original=before.value.and_then(AudioState::preset);
        let allowed=capability.value.as_ref().is_some_and(|c|c.supported.contains(&target)
            && original.is_some_and(|p|c.supported.contains(&p)));
        report.capability=Some(capability);report.before=Some(before.clone());report.original=original;
        let Some(original)=original.filter(|_|allowed) else {
            report.message="Known supported original and target audio presets required; no SET sent".into();return report;
        };
        if target==original { report.message="Validation target already equals original; choose a different fixed preset".into();return report; }
        let trial=self.write_known(target,Some(before));report.target_readback_confirmed=trial.readback_confirmed;
        // Immutable original checkpoint. Always restore after invoking a SET,
        // even on connect/write/read timeout or unavailable/failed trial GET.
        // Do not require a fresh current preset: it may now be unknown.
        report.restore_attempted=true;
        let restore=self.write_known(original,None);report.original_state_restored=restore.readback_confirmed;
        report.setter_acks_confirmed=trial.ack==Some(AudioAck::Confirmed)&&restore.ack==Some(AudioAck::Confirmed);
        report.restore_required=restore.status!=ApplyStatus::Applied;
        report.outcome=if !report.original_state_restored || restore.ack==Some(AudioAck::Rejected) {
            AudioValidationOutcome::RestoreUnconfirmed
        } else if trial.status==ApplyStatus::Applied && restore.status==ApplyStatus::Applied {
            AudioValidationOutcome::Confirmed
        } else if report.target_readback_confirmed && trial.ack!=Some(AudioAck::Rejected) {
            AudioValidationOutcome::ReadbacksConfirmedAcksUnknown
        } else { AudioValidationOutcome::TrialUnconfirmed };
        report.trial=Some(trial);report.restore=Some(restore);
        report.message=match report.outcome {
            AudioValidationOutcome::Confirmed=>"Audio trial and original restore have reviewed ACKs and matching independent GETs",
            AudioValidationOutcome::ReadbacksConfirmedAcksUnknown=>"Target and original GETs match, but setter ACK schema remains unverified; no Applied claim",
            AudioValidationOutcome::RestoreUnconfirmed=>"Original audio restore remains unconfirmed; inspect raw SET/error and GET",
            _=>"Audio trial was unconfirmed; inspect the separately recorded original recovery",
        }.into();report
    }
}

pub struct AudioClient { guard:TargetGuard, policy:WritePolicy }
impl AudioClient {
    pub fn connect()->Result<Self,DeviceError> { Self::connect_with_policy(WritePolicy::ReadOnly) }
    pub fn connect_with_policy(policy:WritePolicy)->Result<Self,DeviceError> {
        let device=acer_device::Device::connect()?;Self::for_device(&device,policy)
    }
    /// Native caller supplies its real device handle, never deserialized model/BIOS claims.
    /// Identity guard runs before any audio network request. This avoids a caller-bool bypass.
    pub fn for_device(device:&acer_device::Device,policy:WritePolicy)->Result<Self,DeviceError> {
        let guard=TargetGuard::from_capabilities(&device.capabilities()?)?;Ok(Self{guard,policy})
    }
    pub fn query(&self)->Result<AudioQuery,DeviceError> {
        let _lock=AUDIO_OPERATION_LOCK.lock().map_err(|e|failure(ErrorKind::Transport,e))?;
        Ok(Controller{transport:Network,guard:self.guard.clone(),policy:self.policy}.query())
    }
    pub fn set(&self,target:AudioPreset)->Result<AudioApplyReport,DeviceError> {
        let _lock=AUDIO_OPERATION_LOCK.lock().map_err(|e|failure(ErrorKind::Transport,e))?;
        Ok(Controller{transport:Network,guard:self.guard.clone(),policy:self.policy}.set(target))
    }
    pub fn validate_restore(&self,target:AudioPreset)->Result<AudioValidationReport,DeviceError> {
        let _lock=AUDIO_OPERATION_LOCK.lock().map_err(|e|failure(ErrorKind::Transport,e))?;
        Ok(Controller{transport:Network,guard:self.guard.clone(),policy:self.policy}.validate_restore(target))
    }
}

#[cfg(test)]mod tests {
    use super::*;
    use std::collections::VecDeque;
    fn capability()->Value { json!({"request":"SUPPORT_AUDIO_MODE","result":0,"data":["MUSIC","MOVIE","VOICE","STRATEGY","RPG","SHOOTER","CUSTOM_AUDIO","AUTOMATIC"]}) }
    fn state(code:u64)->Value { json!({"request":"SOUND_MODE","result":"0","data":{"function":"DTS","mode":code}}) }
    fn exchange(request:FixedRequest,response:Value)->RawExchange { let mut raw=request.empty_exchange();raw.response=Some(response);raw.dispatch_attempted=true;raw.bytes_written=request.frame().unwrap().len();raw }
    fn timeout(request:FixedRequest)->RawExchange { let mut raw=request.empty_exchange();raw.dispatch_attempted=true;raw.error=Some(failure(ErrorKind::Transport,"simulated SET timeout after possible dispatch"));raw }
    struct Fake { replies:VecDeque<(FixedRequest,RawExchange)>, calls:Vec<FixedRequest>, settles:usize }
    impl Transport for Fake {
        fn exchange(&mut self,request:FixedRequest)->RawExchange {
            let(expected,response)=self.replies.pop_front().expect("unexpected extra audio request");
            assert_eq!(request,expected);self.calls.push(request);response
        }
        fn settle(&mut self) { self.settles+=1; }
    }
    fn caps()->Capabilities { Capabilities{model:"Predator PHN16-71".into(),bios:"V1.18".into(),matched_model:true,items:vec![]} }
    fn controller(replies:Vec<(FixedRequest,RawExchange)>,policy:WritePolicy)->Controller<Fake> {
        Controller{transport:Fake{replies:replies.into(),calls:vec![],settles:0},guard:TargetGuard::from_capabilities(&caps()).unwrap(),policy}
    }
    fn ok(request:FixedRequest,value:Value)->(FixedRequest,RawExchange) { (request,exchange(request,value)) }
    fn normal_validation()->Vec<(FixedRequest,RawExchange)> { vec![
        ok(FixedRequest::Capability,capability()),ok(FixedRequest::State,state(1)),
        ok(FixedRequest::Set(AudioPreset::Music),json!({"request":"SOUND_MODE","result":"0"})),
        ok(FixedRequest::State,state(0)),
        ok(FixedRequest::Set(AudioPreset::Movie),json!({"request":"SOUND_MODE","result":"0"})),
        ok(FixedRequest::State,state(1)),
    ] }
    #[test]fn closed_presets_and_fixed_acer_frames() {
        assert_eq!(AudioPreset::ALL.map(AudioPreset::code),[0,1,2,3,4,5,6,10]);
        for preset in AudioPreset::ALL {
            assert_eq!(AudioPreset::from_code(preset.code()),Some(preset));
            let bytes=FixedRequest::Set(preset).frame().unwrap();assert_eq!(&bytes[..4],b"ACER");
            assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()),100);
            assert_eq!(serde_json::from_slice::<Value>(&bytes[8..]).unwrap(),json!({"Function":"SOUND_MODE","Parameter":{"mode":preset.code(),"function":"DTS"}}));
        }
        for unknown in [r#""off""#,r#""9""#,r#"9"#,r#"{"mode":1,"Function":"OTHER"}"#] {
            assert!(serde_json::from_str::<AudioPreset>(unknown).is_err());
        }
        assert_eq!(FixedRequest::Capability.packet_id(),0);assert_eq!(FixedRequest::State.packet_id(),20);
    }
    #[test]fn real_read_fixture_has_distinct_numeric_and_string_result_formats() {
        let fixture:Value=serde_json::from_str(include_str!("../../../evidence/audio-service-read.json").trim_start_matches('\u{feff}')).unwrap();
        assert_eq!(parse_capability(&fixture["reads"][0]["response"]).unwrap().supported,AudioPreset::ALL);
        assert_eq!(parse_state(&fixture["reads"][1]["response"]).unwrap(),AudioState::Available{preset:AudioPreset::Movie});
        let mut raw=capability();raw["result"]=json!("0");assert!(parse_capability(&raw).is_err());
        let mut raw=state(1);raw["result"]=json!(0);assert!(parse_state(&raw).is_err());
        let mut raw=state(1);raw["data"]["function"]=json!("OTHER");assert!(parse_state(&raw).is_err());
    }
    #[test]fn unavailable_nine_is_never_off_or_a_writable_original() {
        assert_eq!(parse_state(&state(9)).unwrap(),AudioState::OutputUnavailable{raw_mode:9});
        for code in [7,8,11,255] { assert!(parse_state(&state(code)).is_err()); }
        for policy in [WritePolicy::ExplicitApply,WritePolicy::SupervisedValidation] {
            let mut c=controller(vec![ok(FixedRequest::Capability,capability()),ok(FixedRequest::State,state(9))],policy);
            assert_eq!(c.set(AudioPreset::Music).status,ApplyStatus::Rejected);
            assert_eq!(c.transport.calls,[FixedRequest::Capability,FixedRequest::State]);
        }
    }
    #[test]fn unknown_setter_formats_are_not_inferred_from_matching_get() {
        let mut replies=normal_validation();
        for(_,raw)in replies.iter_mut().filter(|(request,_)|matches!(request,FixedRequest::Set(_))) {
            raw.response.as_mut().unwrap()["extra"]=json!(0);
        }
        let mut c=controller(replies,WritePolicy::SupervisedValidation);
        let report=c.validate_restore(AudioPreset::Music);
        assert_eq!(report.outcome,AudioValidationOutcome::ReadbacksConfirmedAcksUnknown);
        assert!(report.target_readback_confirmed&&report.original_state_restored&&report.restore_attempted);
        assert!(!report.success()&&!report.setter_acks_confirmed&&report.restore_required);
        assert_eq!(report.trial.unwrap().status,ApplyStatus::Unconfirmed);
        assert_eq!(report.restore.unwrap().ack,Some(AudioAck::Unknown));assert!(c.transport.replies.is_empty());
        for reply in [json!({"request":"SOUND_MODE","result":0}),json!({"request":"SOUND_MODE","result":"0","data":null}),json!({"success":true}),json!(null)] {
            assert_eq!(parse_set_ack(&exchange(FixedRequest::Set(AudioPreset::Music),reply),AudioPreset::Music),AudioAck::Unknown);
        }
    }
    #[test]fn timed_out_trial_always_attempts_original_restore() {
        let mut replies=normal_validation();replies[2].1=timeout(FixedRequest::Set(AudioPreset::Music));
        let mut c=controller(replies,WritePolicy::SupervisedValidation);let report=c.validate_restore(AudioPreset::Music);
        assert!(report.trial.as_ref().unwrap().write.as_ref().unwrap().error.is_some());
        assert!(report.restore_attempted&&report.original_state_restored);
        assert_eq!(c.transport.calls.iter().filter(|r|matches!(r,FixedRequest::Set(_))).count(),2);
    }
    #[test]fn failed_or_unavailable_trial_get_does_not_block_checkpoint_restore() {
        for failure in [Some(state(9)),None] {
            let mut replies=normal_validation();let failed=match failure {
                Some(value)=>exchange(FixedRequest::State,value),None=>timeout(FixedRequest::State),
            };
            replies.splice(3..4,(0..MAX_READBACK_ATTEMPTS).map(|_|(FixedRequest::State,failed.clone())));
            let mut c=controller(replies,WritePolicy::SupervisedValidation);let report=c.validate_restore(AudioPreset::Music);
            assert!(!report.target_readback_confirmed);assert!(report.restore_attempted&&report.original_state_restored);
            assert_eq!(report.outcome,AudioValidationOutcome::TrialUnconfirmed);
        }
    }
    #[test]fn timed_out_restore_and_wrong_final_mode_remain_unconfirmed() {
        let mut replies=normal_validation();replies[4].1=timeout(FixedRequest::Set(AudioPreset::Movie));
        replies.splice(5..6,(0..MAX_READBACK_ATTEMPTS).map(|_|ok(FixedRequest::State,state(0))));
        let mut c=controller(replies,WritePolicy::SupervisedValidation);let report=c.validate_restore(AudioPreset::Music);
        assert!(report.restore_attempted&&report.restore_required);assert!(!report.original_state_restored);
        assert_eq!(report.outcome,AudioValidationOutcome::RestoreUnconfirmed);assert!(c.transport.replies.is_empty());
    }
    #[test]fn unsupported_target_unknown_original_and_readonly_never_set() {
        let mut support=capability();support["data"]=json!(["MOVIE","UNREVIEWED"]);
        let mut c=controller(vec![ok(FixedRequest::Capability,support),ok(FixedRequest::State,state(1))],WritePolicy::SupervisedValidation);
        assert_eq!(c.validate_restore(AudioPreset::Music).outcome,AudioValidationOutcome::Rejected);
        let mut c=controller(vec![],WritePolicy::ReadOnly);assert_eq!(c.set(AudioPreset::Music).status,ApplyStatus::Rejected);
        assert_eq!(c.validate_restore(AudioPreset::Music).outcome,AudioValidationOutcome::Rejected);assert!(c.transport.calls.is_empty());
        let mut c=controller(vec![],WritePolicy::ExplicitApply);assert_eq!(c.validate_restore(AudioPreset::Music).outcome,AudioValidationOutcome::Rejected);assert!(c.transport.calls.is_empty());
    }
    #[test]fn model_bios_guard_and_response_bounds_are_explicit() {
        for(model,bios,matched)in [("PHN16-72","V1.18",true),("PHN16-71","V1.19",true),("PHN16-71","V1.18",false)] {
            let mut caps=caps();caps.model=model.into();caps.bios=bios.into();caps.matched_model=matched;
            assert!(TargetGuard::from_capabilities(&caps).is_err());
        }
        assert!(complete_response(&vec![b' ';MAX_RESPONSE+1]).is_err());
        assert!(complete_response(br#"{"request":"SOUND_MODE""#).unwrap().is_none());
        assert!(complete_response(br#"{"result":0}junk"#).is_err());
        assert!(remaining(Instant::now()-Duration::from_millis(1)).is_err());
    }
    fn fixture_exchange(value:&Value)->RawExchange {
        RawExchange{packet_id:value["packet_id"].as_u64().unwrap() as u32,request:value["request"].clone(),
            response:if value["response"].is_null(){None}else{Some(value["response"].clone())},
            dispatch_attempted:value["dispatch_attempted"].as_bool().unwrap(),
            bytes_written:value["bytes_written"].as_u64().unwrap() as usize,
            bytes_received:value["bytes_received"].as_u64().unwrap() as usize,
            error:if value["error"].is_null(){None}else{Some(serde_json::from_value(value["error"].clone()).unwrap())}}
    }
    #[test]fn actual_set_fixture_confirms_only_acks_and_keeps_original_stale_get_evidence() {
        let first:Value=serde_json::from_str(include_str!("../../../evidence/audio-music-first-restore.json").trim_start_matches('\u{feff}')).unwrap();
        // This historical JSON remains a failed immediate-readback run. ACK
        // recognition must not rewrite it or reinterpret its old GET as success.
        assert_eq!(first["outcome"],"restore_unconfirmed");
        assert_eq!(parse_set_ack(&fixture_exchange(&first["trial"]["write"]),AudioPreset::Music),AudioAck::Confirmed);
        assert_eq!(parse_set_ack(&fixture_exchange(&first["restore"]["write"]),AudioPreset::Movie),AudioAck::Confirmed);
        assert_eq!(parse_state(&first["trial"]["after"]["raw"]["response"]).unwrap(),AudioState::Available{preset:AudioPreset::Movie});
        assert_eq!(parse_state(&first["restore"]["after"]["raw"]["response"]).unwrap(),AudioState::Available{preset:AudioPreset::Music});
        let later:Value=serde_json::from_str(include_str!("../../../evidence/audio-after-first-restore.json").trim_start_matches('\u{feff}')).unwrap();
        assert_eq!(parse_state(&later["state"]["raw"]["response"]).unwrap(),AudioState::Available{preset:AudioPreset::Movie});
    }
    #[test]fn reviewed_set_ack_rejects_bad_request_and_does_not_accept_extra_or_malformed_fields() {
        let request=FixedRequest::Set(AudioPreset::Music);
        let good=exchange(request,json!({"request":"SOUND_MODE","result":"0"}));
        assert_eq!(parse_set_ack(&good,AudioPreset::Music),AudioAck::Confirmed);
        assert_eq!(parse_set_ack(&good,AudioPreset::Movie),AudioAck::Rejected);
        let mut raw=good.clone();raw.packet_id=20;assert_eq!(parse_set_ack(&raw,AudioPreset::Music),AudioAck::Rejected);
        let mut raw=good.clone();raw.request["Parameter"]["mode"]=json!(9);assert_eq!(parse_set_ack(&raw,AudioPreset::Music),AudioAck::Rejected);
        let mut raw=good.clone();raw.request["Parameter"]["function"]=json!("OTHER");assert_eq!(parse_set_ack(&raw,AudioPreset::Music),AudioAck::Rejected);
        let mut raw=good.clone();raw.bytes_written-=1;assert_eq!(parse_set_ack(&raw,AudioPreset::Music),AudioAck::Unknown);
        let mut raw=good.clone();raw.dispatch_attempted=false;assert_eq!(parse_set_ack(&raw,AudioPreset::Music),AudioAck::Unknown);
        for result in [json!("1"),json!("-1"),json!(2)] {
            assert_eq!(parse_set_ack(&exchange(request,json!({"request":"SOUND_MODE","result":result})),AudioPreset::Music),AudioAck::Rejected);
        }
        for result in [json!(0),json!(false),json!(null),json!("00"),json!("unexpected")] {
            assert_eq!(parse_set_ack(&exchange(request,json!({"request":"SOUND_MODE","result":result})),AudioPreset::Music),AudioAck::Unknown);
        }
        for reply in [json!({"request":"SOUND_MODE","result":"0","extra":0}),json!({"request":"SOUND_MODE"}),json!(["SOUND_MODE","0"])] {
            assert_eq!(parse_set_ack(&exchange(request,reply),AudioPreset::Music),AudioAck::Unknown);
        }
        assert_eq!(parse_set_ack(&exchange(request,json!({"request":"OTHER","result":"0"})),AudioPreset::Music),AudioAck::Rejected);
    }
    #[test]fn asynchronous_trial_and_restore_wait_for_old_then_new_without_repeating_set() {
        let replies=vec![ok(FixedRequest::Capability,capability()),ok(FixedRequest::State,state(1)),
            ok(FixedRequest::Set(AudioPreset::Music),json!({"request":"SOUND_MODE","result":"0"})),
            ok(FixedRequest::State,state(1)),ok(FixedRequest::State,state(0)),
            ok(FixedRequest::Set(AudioPreset::Movie),json!({"request":"SOUND_MODE","result":"0"})),
            ok(FixedRequest::State,state(0)),ok(FixedRequest::State,state(1))];
        let mut c=controller(replies,WritePolicy::SupervisedValidation);let report=c.validate_restore(AudioPreset::Music);
        assert!(report.success()&&report.setter_acks_confirmed&&report.original_state_restored);
        assert!(!report.restore_required);assert_eq!(c.transport.settles,2);
        let trial=report.trial.unwrap();let restore=report.restore.unwrap();
        assert_eq!(trial.status,ApplyStatus::Applied);assert_eq!(restore.status,ApplyStatus::Applied);
        assert_eq!(trial.readback_attempts.len(),2);assert_eq!(restore.readback_attempts.len(),2);
        assert_eq!(trial.readback_attempts[0].value,Some(AudioState::Available{preset:AudioPreset::Movie}));
        assert_eq!(restore.readback_attempts[0].value,Some(AudioState::Available{preset:AudioPreset::Music}));
        assert_eq!(c.transport.calls.iter().filter(|r|matches!(r,FixedRequest::Set(_))).count(),2);
        assert!(c.transport.replies.is_empty());
    }
    #[test]fn production_set_stops_after_four_old_unavailable_or_unknown_gets() {
        for code in [1,9,255] {
            let mut replies=vec![ok(FixedRequest::Capability,capability()),ok(FixedRequest::State,state(1)),
                ok(FixedRequest::Set(AudioPreset::Music),json!({"request":"SOUND_MODE","result":"0"}))];
            replies.extend((0..MAX_READBACK_ATTEMPTS).map(|_|ok(FixedRequest::State,state(code))));
            let mut c=controller(replies,WritePolicy::ExplicitApply);let report=c.set(AudioPreset::Music);
            assert_eq!(report.status,ApplyStatus::Unconfirmed);assert_eq!(report.ack,Some(AudioAck::Confirmed));
            assert!(!report.readback_confirmed);assert_eq!(report.readback_attempts.len(),MAX_READBACK_ATTEMPTS);
            assert_eq!(c.transport.settles,MAX_READBACK_ATTEMPTS-1);
            assert_eq!(c.transport.calls.iter().filter(|r|matches!(r,FixedRequest::Set(_))).count(),1);
            if code==255 { assert!(report.readback_attempts.iter().all(|r|r.error.is_some())); }
            assert!(c.transport.replies.is_empty());
        }
    }
    #[test]fn acknowledged_restore_also_waits_but_keeps_stale_final_mode_unconfirmed() {
        let mut replies=normal_validation();
        replies.splice(5..6,(0..MAX_READBACK_ATTEMPTS).map(|_|ok(FixedRequest::State,state(0))));
        let mut c=controller(replies,WritePolicy::SupervisedValidation);let report=c.validate_restore(AudioPreset::Music);
        assert_eq!(report.outcome,AudioValidationOutcome::RestoreUnconfirmed);assert!(report.restore_required);
        let restore=report.restore.unwrap();assert_eq!(restore.ack,Some(AudioAck::Confirmed));
        assert_eq!(restore.readback_attempts.len(),MAX_READBACK_ATTEMPTS);
        assert_eq!(c.transport.calls.iter().filter(|r|matches!(r,FixedRequest::Set(_))).count(),2);
    }
    #[test]fn query_reports_verified_ack_schema_but_requires_known_supported_original() {
        for(policy,code,writable)in [(WritePolicy::ReadOnly,1,false),(WritePolicy::ExplicitApply,1,true),(WritePolicy::ExplicitApply,9,false)] {
            let mut c=controller(vec![ok(FixedRequest::Capability,capability()),ok(FixedRequest::State,state(code))],policy);
            let report=c.query();assert!(report.setter_ack_schema_verified);assert_eq!(report.writable,writable);
        }
        let mut support=capability();support["data"]=json!(["MUSIC"]);
        let mut c=controller(vec![ok(FixedRequest::Capability,support.clone()),ok(FixedRequest::State,state(1))],WritePolicy::ExplicitApply);
        assert!(!c.query().writable);
        let mut c=controller(vec![ok(FixedRequest::Capability,support),ok(FixedRequest::State,state(1))],WritePolicy::ExplicitApply);
        assert_eq!(c.set(AudioPreset::Music).status,ApplyStatus::Rejected);
        assert!(c.transport.calls.iter().all(|r|!matches!(r,FixedRequest::Set(_))));
    }
    #[test]fn fixed_deadlines_and_settle_counts_fit_helper_network_budget() {
        assert!(FixedRequest::Capability.timeout()<=Duration::from_secs(1));
        assert!(FixedRequest::State.timeout()<=Duration::from_secs(1));
        assert_eq!(FixedRequest::Set(AudioPreset::Music).timeout(),Duration::from_secs(3));
        assert_eq!(MAX_READBACK_ATTEMPTS,4);assert_eq!(SETTLE_INTERVAL,Duration::from_millis(100));
        let normal_budget=READ_TIMEOUT*6+SET_TIMEOUT+SETTLE_INTERVAL*3;
        let validation_budget=READ_TIMEOUT*10+SET_TIMEOUT*2+SETTLE_INTERVAL*6;
        assert_eq!(normal_budget,Duration::from_millis(7800));
        assert_eq!(validation_budget,Duration::from_millis(14100));
        assert!(normal_budget<Duration::from_secs(15));assert!(validation_budget<Duration::from_secs(15));
    }
}
