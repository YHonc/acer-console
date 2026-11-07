//! Fixed, reviewed Acer WMI vocabulary. No arbitrary method execution API.
pub mod rapl;
pub mod ipf_power;
pub mod ipf_ppcc;
pub mod rapl_plan;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum RawValue { Bool(bool), Unsigned(String), Signed(String), Text(String), Bytes(Vec<u8>), Array(Vec<RawValue>), Null }
impl RawValue {
    pub fn unsigned(n: u64) -> Self { Self::Unsigned(n.to_string()) }
    pub fn as_u64(&self) -> Option<u64> { match self { Self::Unsigned(s) => s.parse().ok(), _ => None } }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceError { pub kind: ErrorKind, pub message: String }
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all="snake_case")]
pub enum ErrorKind { PermissionDenied, Unsupported, Transport, Protocol, WorkerStopped, InvalidArgument }
impl std::fmt::Display for DeviceError { fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f,"{:?}: {}", self.kind, self.message) } }
impl std::error::Error for DeviceError {}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawRead { pub class: String, pub method: String, pub input: BTreeMap<String, RawValue>, pub output: BTreeMap<String, RawValue>, pub error: Option<DeviceError> }
impl RawRead {
    /// The PHN16-71 provider omits ReturnValue from real getter output despite its
    /// Boolean metadata. Accept absent returns only for reviewed getter schemas.
    /// An explicit false/non-Boolean return or transport error is never accepted.
    pub fn transport_ok(&self) -> bool {
        if self.error.is_some(){return false;}
        match self.output.get("ReturnValue"){
            Some(RawValue::Bool(true))=>true,
            Some(_)=>false,
            None=>match (self.class.as_str(),self.method.as_str()){
                ("AcerGamingFunction","GetGamingSysInfo"|"GetGamingMiscSetting"|"GetGamingFanBehavior"|"GetGamingFanSpeed"|"GetGamingRgbKb"|"GetGamingLEDBehavior"|"GetGamingLEDColor")=>matches!(self.output.get("gmOutput"),Some(RawValue::Unsigned(_))),
                ("AcerGamingFunction","GetGamingKBBacklight")=>matches!(self.output.get("gmOutput"),Some(RawValue::Bytes(v)) if v.len()==15)&&self.output.get("gmReturn").and_then(RawValue::as_u64)==Some(0),
                ("BatteryControl","GetBatteryHealthControlStatus")=>matches!(self.output.get("uFunctionStatus"),Some(RawValue::Bytes(v)) if v.len()==5)&&matches!(self.output.get("uReturn"),Some(RawValue::Bytes(v)) if v.len()==2)&&self.output.get("uFunctionList").and_then(RawValue::as_u64).is_some_and(|v|v<=255),
                _=>false,
            }
        }
    }
    pub fn scalar(&self) -> Option<u64> { if self.transport_ok() { self.output.get("gmOutput")?.as_u64().filter(|v|v&255==0) } else { None } }
    pub fn firmware_write_ok(&self)->Option<bool>{
        if self.error.is_some()||self.output.get("ReturnValue")==Some(&RawValue::Bool(false)){return Some(false);}
        // An omitted transport return cannot hide an explicit firmware rejection.
        // Zero without ReturnValue remains unconfirmed until supervised readback.
        if self.class=="AcerGamingFunction"&&self.method.starts_with("Set")&&self.output.get("gmOutput").and_then(RawValue::as_u64).is_some_and(|v|v&255!=0){return Some(false);}
        if self.class=="BatteryControl"&&self.method=="SetBatteryHealthControl"&&self.output.get("uReturn").and_then(RawValue::as_u64).is_some_and(|v|v!=0){return Some(false);}
        if !self.transport_ok(){return None;}
        if self.class=="BatteryControl"{self.output.get("uReturn")?.as_u64().map(|v|v==0)}else{self.output.get("gmOutput")?.as_u64().map(|v|v&255==0)}
    }
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all="snake_case")]
pub enum Profile { Quiet, Balanced, Performance, Turbo, Eco }
impl Profile {
    pub fn code(self) -> u8 { match self { Self::Quiet=>0,Self::Balanced=>1,Self::Performance=>4,Self::Turbo=>5,Self::Eco=>6 } }
    pub fn decode(raw: u64) -> Option<Self> { if raw > 0xffff { return None; } match (raw >> 8) & 255 { 0=>Some(Self::Quiet),1=>Some(Self::Balanced),4=>Some(Self::Performance),5=>Some(Self::Turbo),6=>Some(Self::Eco),_=>None } }
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all="snake_case")]
pub enum FanMode { Auto, Max, Custom }
/// Independent fixed-source implementations agree: CPU uses bits8..9, GPU bits14..15.
/// Both reply status bytes must be zero; mixed/unknown states stay unknown.
pub fn fan_mode(cpu: u64, gpu: u64) -> Option<FanMode> {
    if cpu&255!=0||gpu&255!=0{return None;}
    let decode=|value:u64|match value{1=>Some(FanMode::Auto),2=>Some(FanMode::Max),3=>Some(FanMode::Custom),_=>None};
    let cpu=decode((cpu>>8)&3)?;let gpu=decode((gpu>>14)&3)?;if cpu==gpu {Some(cpu)} else {None}
}
pub fn sensor_value(raw:u64)->Option<u16>{if raw&255==0{Some(((raw>>8)&0xffff) as u16)}else{None}}
pub fn sensor_supported(mask:u64,id:u8)->bool { mask&255==0 && (1..=40).contains(&id) && (mask>>24)&(1u64<<(id-1))!=0 }
pub fn temperature(raw:u64)->Option<f64>{sensor_value(raw).filter(|n|(1..=125).contains(n)).map(f64::from)}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Lighting { pub brightness: u8, pub zones: [[u8;3];4] }
#[derive(Debug,Clone,Serialize,Deserialize,PartialEq,Eq)]
#[serde(deny_unknown_fields)]
pub struct LightingAnimation{pub effect:LightingEffect,pub speed:u8,pub brightness:u8,pub direction:u8,pub color:[u8;3]}
#[derive(Debug,Clone,Serialize,Deserialize,PartialEq,Eq)]
pub struct LightingConfig{pub effect:LightingEffect,pub speed:u8,pub brightness:u8,pub direction:u8,pub color:[u8;3],pub zones:Option<[[u8;3];4]>}
#[derive(Debug,Clone,Copy,Serialize,Deserialize,PartialEq,Eq)]#[serde(rename_all="snake_case")]
pub enum LightingEffect{Static,Breathing,Neon,Wave,Shifting,Zoom,Meteor,Twinkling}
impl LightingEffect{pub fn code(self)->u8{match self{Self::Static=>0,Self::Breathing=>1,Self::Neon=>2,Self::Wave=>3,Self::Shifting=>4,Self::Zoom=>5,Self::Meteor=>6,Self::Twinkling=>7}}pub fn from_code(code:u8)->Option<Self>{match code{0=>Some(Self::Static),1=>Some(Self::Breathing),2=>Some(Self::Neon),3=>Some(Self::Wave),4=>Some(Self::Shifting),5=>Some(Self::Zoom),6=>Some(Self::Meteor),7=>Some(Self::Twinkling),_=>None}}}
impl LightingEffect{pub fn uses_color(self)->bool{!matches!(self,Self::Static|Self::Neon|Self::Wave)}pub fn uses_direction(self)->bool{matches!(self,Self::Wave|Self::Shifting)}}
pub fn valid_animation(target:&LightingAnimation)->bool{target.effect!=LightingEffect::Static&&(1..=9).contains(&target.speed)&&target.brightness<=100&&if target.effect.uses_direction(){(1..=2).contains(&target.direction)}else{target.direction<=2}}
pub fn animation_frame(target:&LightingAnimation)->Result<Vec<u8>,DeviceError>{if !valid_animation(target){return Err(DeviceError{kind:ErrorKind::InvalidArgument,message:"Animation requires effect1..7, speed1..9, brightness0..100 and right1/left2 for directional effects".into()});}backlight_payload(target.effect.code(),target.speed,target.brightness,if target.direction==0{1}else{target.direction},target.color)}
pub const DYNAMIC_TRIAL_COLOR:[u8;3]=[255,128,0];
pub fn dynamic_trial_frame(effect:LightingEffect)->Option<Vec<u8>>{if effect==LightingEffect::Static{return None;}backlight_payload(effect.code(),3,20,1,DYNAMIC_TRIAL_COLOR).ok()}
pub fn dynamic_restore_plan(saved:&BacklightFrame)->Option<Vec<Call>>{let mut calls=vec![backlight_frame_call(brightness_frame(saved,0)?)];calls.extend(static_color_restore_plan(saved)?);Some(calls)}
pub fn dynamic_trial_matches(snapshot:&Snapshot,effect:LightingEffect)->bool{
    if effect==LightingEffect::Static||snapshot.lighting_effect!=Some(effect){return false;}
    let Some(read)=snapshot.reads.iter().find(|r|r.class=="AcerGamingFunction"&&r.method=="GetGamingKBBacklight"&&r.input.get("gmInput").and_then(RawValue::as_u64)==Some(1))else{return false;};
    if backlight_effect(read)!=Some(effect){return false;}let Some(RawValue::Bytes(state))=read.output.get("gmOutput")else{return false;};
    if state[1]!=3||state[2]!=20||state[3]!=0{return false;}
    if matches!(effect,LightingEffect::Wave|LightingEffect::Shifting)&&state[4]!=1{return false;}
    if !matches!(effect,LightingEffect::Neon|LightingEffect::Wave)&&state[5..8]!=DYNAMIC_TRIAL_COLOR{return false;}
    true
}
pub fn backlight_effect(read:&RawRead)->Option<LightingEffect>{if read.class!="AcerGamingFunction"||read.method!="GetGamingKBBacklight"||!read.transport_ok()||read.output.get("gmReturn").and_then(RawValue::as_u64)!=Some(0){return None;}let RawValue::Bytes(state)=read.output.get("gmOutput")?else{return None;};if state.len()!=15||state[8]!=3{return None;}LightingEffect::from_code(state[0])}
#[derive(Debug,Clone,Serialize,Deserialize,PartialEq,Eq)]
pub struct BacklightFrame {pub observed_state:Vec<u8>,pub setter_frame:Vec<u8>,pub reconstructed:bool,pub gm_return:u8,pub zone_words:Vec<String>,pub mapping:String}
#[derive(Debug,Clone,Serialize,Deserialize,PartialEq,Eq)]
pub struct BatteryCareState{pub enabled:bool,pub calibrating:bool,pub function_list:u8,pub function_status:Vec<u8>,pub return_status:Vec<u8>}
pub fn battery_health_state(read:&RawRead)->Option<BatteryCareState>{
    if read.class!="BatteryControl"||read.method!="GetBatteryHealthControlStatus"||!read.transport_ok(){return None;}
    let list=u8::try_from(read.output.get("uFunctionList")?.as_u64()?).ok()?;if list&1==0{return None;}
    let RawValue::Bytes(status)=read.output.get("uFunctionStatus")?else{return None;};let RawValue::Bytes(result)=read.output.get("uReturn")?else{return None;};
    if status.len()!=5||result.as_slice()!=[0,0]||status[0]>1||status[1]>1{return None;}
    Some(BatteryCareState{enabled:status[0]==1,calibrating:status[1]==1,function_list:list,function_status:status.clone(),return_status:result.clone()})
}
pub fn battery_validation_state(snapshot:&Snapshot)->Option<BatteryCareState>{let call=battery_read();let read=snapshot.reads.iter().find(|r|r.class==call.class&&r.method==call.method&&r.input==call.input)?;let state=battery_health_state(read)?;if snapshot.battery_care!=Some(state.enabled)||state.calibrating||state.function_status[1..].iter().any(|v|*v!=0){return None;}Some(state)}
pub fn battery_care_readback(snapshot:&Snapshot,saved:&BatteryCareState,target:bool)->bool{let Some(current)=battery_validation_state(snapshot)else{return false;};current.enabled==target&&current.function_list==saved.function_list&&current.function_status[1..]==saved.function_status[1..]}
pub fn battery_care_ack(read:&RawRead,target:bool)->Option<bool>{let call=encode(&Command::SetBatteryCare{enabled:target}).ok()?.remove(0);if read.error.is_some()||read.class!=call.class||read.method!=call.method||read.input!=call.input{return Some(false);}match read.output.get("ReturnValue"){None|Some(RawValue::Bool(true))=>{},_=>return Some(false)}read.output.get("uReturn")?.as_u64().map(|result|result==0)}
pub fn verified_battery_result(target:bool,saved:&BatteryCareState,writes:&[RawRead],after:Option<&Snapshot>)->ApplyStatus{if writes.len()!=1{return ApplyStatus::Unconfirmed;}match battery_care_ack(&writes[0],target){Some(false)=>ApplyStatus::Failed,Some(true)if after.is_some_and(|s|battery_care_readback(s,saved,target))=>ApplyStatus::Applied,_=>ApplyStatus::Unconfirmed}}
pub fn observed_backlight_frame(snapshot:&Snapshot)->Option<BacklightFrame>{
    let read=|method:&str,selector:u64|snapshot.reads.iter().find(|r|r.class=="AcerGamingFunction"&&r.method==method&&r.input.get("gmInput").and_then(RawValue::as_u64)==Some(selector));
    let backlight=read("GetGamingKBBacklight",1)?;
    if !backlight.transport_ok()||backlight.output.get("gmReturn").and_then(RawValue::as_u64)!=Some(0){return None;}
    let RawValue::Bytes(state)=backlight.output.get("gmOutput")? else{return None;};
    if state.len()!=15||state[0]!=0||state[2]>100||state[3]!=0||state[8]!=3||state[9]>1||state[10..].iter().any(|v|*v!=0){return None;}
    let mut zones=vec![];for selector in [1,2,4,8]{zones.push(read("GetGamingRgbKb",selector)?.scalar()?.to_string());}
    let mut setter=state.clone();setter.resize(16,0);setter[9]=1;
    Some(BacklightFrame{observed_state:state.clone(),setter_frame:setter,reconstructed:true,gm_return:0,zone_words:zones,mapping:"Getter exposes15 state bytes plus independent gmReturn. Local DLL sets byte9=1 for keyboard routing and byte15=0 padding. Normalize byte9 on readback; never append gmReturn to payload.".into()})
}
fn observed_lighting_frame_reads(reads:&[RawRead])->Option<BacklightFrame>{
    let read=|method:&str,selector:u64|reads.iter().find(|r|r.class=="AcerGamingFunction"&&r.method==method&&r.input.get("gmInput").and_then(RawValue::as_u64)==Some(selector));let backlight=read("GetGamingKBBacklight",1)?;let effect=backlight_effect(backlight)?;let RawValue::Bytes(state)=backlight.output.get("gmOutput")?else{return None;};
    if state[2]>100||state[3]!=0||state[9]>1||state[10..].iter().any(|v|*v!=0)||state[1]>9||(effect!=LightingEffect::Static&&state[1]==0)||state[4]>2||(effect.uses_direction()&&state[4]==0){return None;}
    let mut zone_words=vec![];for selector in [1,2,4,8]{if let Some(value)=read("GetGamingRgbKb",selector).and_then(RawRead::scalar){zone_words.push(value.to_string());}else{zone_words.clear();break;}}
    let mut setter_frame=state.clone();setter_frame.resize(16,0);setter_frame[9]=1;
    Some(BacklightFrame{observed_state:state.clone(),setter_frame,reconstructed:true,gm_return:0,zone_words,mapping:"Actual getter15 with independent gmReturn; setter16 reconstructs keyboard route byte9=1 and padding byte15=0. Empty zone_words means unavailable; dynamic zone reads are not historical static presets.".into()})
}
pub fn observed_lighting_frame(snapshot:&Snapshot)->Option<BacklightFrame>{observed_lighting_frame_reads(&snapshot.reads)}
pub fn lighting_config_from_reads(reads:&[RawRead])->Option<LightingConfig>{let frame=observed_lighting_frame_reads(reads)?;let state=&frame.observed_state;let effect=LightingEffect::from_code(state[0])?;let zones=(||{if effect!=LightingEffect::Static||frame.zone_words.len()!=4{return None;}let mut colors=[[0;3];4];for(i,word)in frame.zone_words.iter().enumerate(){let value=word.parse::<u64>().ok()?;if value>0xffff_ff00{return None;}colors[i]=[((value>>8)&255)as u8,((value>>16)&255)as u8,((value>>24)&255)as u8];}Some(colors)})();Some(LightingConfig{effect,speed:state[1],brightness:state[2],direction:state[4],color:[state[5],state[6],state[7]],zones})}
pub fn static_frame(saved:&BacklightFrame,brightness:u8)->Option<Vec<u8>>{if brightness>100||saved.setter_frame.len()!=16{return None;}let mut frame=saved.setter_frame.clone();frame[0]=0;frame[1]=0;frame[2]=brightness;frame[3]=0;frame[4]=0;frame[8]=3;frame[9]=1;frame[15]=0;Some(frame)}
pub fn lighting_restore_plan(saved:&BacklightFrame)->Option<Vec<Call>>{let effect=LightingEffect::from_code(*saved.observed_state.first()?)?;let mut calls=vec![backlight_frame_call(static_frame(saved,0)?)];if let Some(zone_plan)=static_color_restore_plan(saved){calls.extend(zone_plan);}else if effect!=LightingEffect::Static&&saved.zone_words.is_empty(){calls.push(backlight_frame_call(saved.setter_frame.clone()));}else{return None;}Some(calls)}
pub fn lighting_control_ready(snapshot:&Snapshot)->bool{observed_lighting_frame(snapshot).is_some_and(|f|lighting_restore_plan(&f).is_some())}
pub fn animation_readback(snapshot:&Snapshot,target:&LightingAnimation)->bool{let Some(current)=lighting_config_from_reads(&snapshot.reads)else{return false;};current.effect==target.effect&&current.speed==target.speed&&current.brightness==target.brightness&&(!target.effect.uses_direction()||current.direction==target.direction)&&(!target.effect.uses_color()||current.color==target.color)}
pub fn verified_animation_result(target:&LightingAnimation,writes:&[RawRead],after:Option<&Snapshot>)->ApplyStatus{let Ok(frame)=animation_frame(target)else{return ApplyStatus::Rejected;};if writes.len()!=1{return ApplyStatus::Unconfirmed;}match backlight_frame_ack(&writes[0],&frame){Some(false)=>ApplyStatus::Failed,Some(true)if after.is_some_and(|s|animation_readback(s,target))=>ApplyStatus::Applied,_=>ApplyStatus::Unconfirmed}}
pub fn lighting_restore_matches(snapshot:&Snapshot,saved:&BacklightFrame)->bool{let Some(effect)=saved.observed_state.first().and_then(|v|LightingEffect::from_code(*v))else{return false;};if effect==LightingEffect::Static{return backlight_frame_matches(snapshot,saved,saved.observed_state[2]);}let target=LightingAnimation{effect,speed:saved.observed_state[1],brightness:saved.observed_state[2],direction:saved.observed_state[4],color:[saved.observed_state[5],saved.observed_state[6],saved.observed_state[7]]};animation_readback(snapshot,&target)}
pub fn verified_lighting_restore_result(saved:&BacklightFrame,writes:&[RawRead],after:Option<&Snapshot>)->ApplyStatus{let Some(calls)=lighting_restore_plan(saved)else{return ApplyStatus::Rejected;};if calls.iter().zip(writes).any(|(c,r)|lighting_call_ack(c,r)==Some(false)){return ApplyStatus::Failed;}if calls.len()!=writes.len()||!calls.iter().zip(writes).all(|(c,r)|lighting_call_ack(c,r)==Some(true)){return ApplyStatus::Unconfirmed;}if after.is_some_and(|s|lighting_restore_matches(s,saved)){ApplyStatus::Applied}else{ApplyStatus::Unconfirmed}}
pub fn brightness_frame(frame:&BacklightFrame,brightness:u8)->Option<Vec<u8>>{if brightness>100||frame.setter_frame.len()!=16||frame.setter_frame[0]!=0||frame.setter_frame[8]!=3||frame.setter_frame[9]!=1||frame.setter_frame[15]!=0{return None;}let mut bytes=frame.setter_frame.clone();bytes[2]=brightness;Some(bytes)}
pub fn backlight_frame_matches(snapshot:&Snapshot,saved:&BacklightFrame,brightness:u8)->bool{let Some(current)=observed_backlight_frame(snapshot)else{return false;};brightness_frame(saved,brightness).is_some_and(|expected|current.setter_frame==expected&&current.zone_words==saved.zone_words)}
pub fn backlight_frame_call(frame:Vec<u8>)->Call{Call{class:"AcerGamingFunction".into(),method:"SetGamingKBBacklight".into(),input:BTreeMap::from([("gmInput".into(),RawValue::Bytes(frame))])}}
pub fn backlight_frame_ack(read:&RawRead,frame:&[u8])->Option<bool>{
    if read.error.is_some(){return Some(false);}
    if read.class!="AcerGamingFunction"||read.method!="SetGamingKBBacklight"||read.input.len()!=1||read.input.get("gmInput")!=Some(&RawValue::Bytes(frame.to_vec())){return Some(false);}
    match read.output.get("ReturnValue"){None|Some(RawValue::Bool(true))=>{},_=>return Some(false)}
    read.output.get("gmOutput")?.as_u64().map(|status|status==0)
}
pub const VALIDATION_STATIC_ZONES:[[u8;3];4]=[[255,0,0],[0,255,0],[0,0,255],[255,255,255]];
pub fn zone_color_word(zone:usize,color:[u8;3])->Option<u64>{if zone>=4{return None;}Some((1u64<<zone)|((color[0]as u64)<<8)|((color[1]as u64)<<16)|((color[2]as u64)<<24))}
pub fn static_color_trial_plan(saved:&BacklightFrame)->Option<Vec<Call>>{let mut calls=vec![backlight_frame_call(brightness_frame(saved,20)?)];for(i,color)in VALIDATION_STATIC_ZONES.into_iter().enumerate(){calls.push(gaming("SetGamingRgbKb",zone_color_word(i,color)?));}Some(calls)}
pub fn static_color_restore_plan(saved:&BacklightFrame)->Option<Vec<Call>>{if saved.zone_words.len()!=4{return None;}let mut calls=vec![];for(i,word)in saved.zone_words.iter().enumerate(){let word=word.parse::<u64>().ok()?;if word>0xffff_ff00||word&255!=0{return None;}calls.push(gaming("SetGamingRgbKb",word|(1u64<<i)));}calls.push(backlight_frame_call(saved.setter_frame.clone()));Some(calls)}
pub fn lighting_call_ack(call:&Call,read:&RawRead)->Option<bool>{if call.class!="AcerGamingFunction"||!matches!(call.method.as_str(),"SetGamingRgbKb"|"SetGamingKBBacklight")||read.class!=call.class||read.method!=call.method||read.input!=call.input||read.error.is_some(){return Some(false);}match read.output.get("ReturnValue"){None|Some(RawValue::Bool(true))=>{},_=>return Some(false)}read.output.get("gmOutput")?.as_u64().map(|status|status==0)}
pub fn static_color_trial_matches(snapshot:&Snapshot,saved:&BacklightFrame)->bool{
    let Some(mut current)=observed_backlight_frame(snapshot)else{return false;};let Some(expected)=brightness_frame(saved,20)else{return false;};
    let target_words:Vec<_>=VALIDATION_STATIC_ZONES.into_iter().enumerate().map(|(i,c)|(zone_color_word(i,c).unwrap()&!255).to_string()).collect();
    // The three color-cache bytes may follow a zone write. Preserve and compare
    // every non-color setting, and verify all four intended zone colors directly.
    current.setter_frame[5..8].copy_from_slice(&expected[5..8]);current.setter_frame==expected&&current.zone_words==target_words
}
pub fn static_lighting_plan(saved:&BacklightFrame,target:&Lighting)->Option<Vec<Call>>{let mut calls=vec![backlight_frame_call(static_frame(saved,target.brightness)?)];for(i,color)in target.zones.into_iter().enumerate(){calls.push(gaming("SetGamingRgbKb",zone_color_word(i,color)?));}Some(calls)}
pub fn verified_static_ready(snapshot:&Snapshot,target:&Lighting)->bool{target.brightness<=100&&lighting_control_ready(snapshot)}
pub fn static_lighting_matches(snapshot:&Snapshot,saved:&BacklightFrame,target:&Lighting)->bool{let Some(mut current)=observed_backlight_frame(snapshot)else{return false;};let Some(expected)=static_frame(saved,target.brightness)else{return false;};let target_words:Vec<_>=target.zones.into_iter().enumerate().map(|(i,c)|(zone_color_word(i,c).unwrap()&!255).to_string()).collect();current.setter_frame[5..8].copy_from_slice(&expected[5..8]);current.setter_frame==expected&&current.zone_words==target_words&&snapshot.lighting.as_ref()==Some(target)}
pub fn verified_static_result(target:&Lighting,saved:&BacklightFrame,writes:&[RawRead],after:Option<&Snapshot>)->ApplyStatus{let Some(calls)=static_lighting_plan(saved,target)else{return ApplyStatus::Rejected;};if calls.iter().zip(writes).any(|(c,r)|lighting_call_ack(c,r)==Some(false)){return ApplyStatus::Failed;}if writes.len()!=calls.len()||!calls.iter().zip(writes).all(|(c,r)|lighting_call_ack(c,r)==Some(true)){return ApplyStatus::Unconfirmed;}if after.is_some_and(|s|static_lighting_matches(s,saved,target)){ApplyStatus::Applied}else{ApplyStatus::Unconfirmed}}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag="type", rename_all="snake_case",deny_unknown_fields)]
pub enum Command { SetProfile { profile: Profile }, SetFanMode { mode: FanMode }, SetFanSpeeds { cpu_percent: u8, gpu_percent: u8 }, SetLighting { lighting: Lighting }, SetLightingAnimation{lighting:LightingAnimation},RestoreLighting{},SetBatteryCare { enabled: bool } }
impl Command { pub fn capability(&self) -> &'static str { match self { Self::SetProfile{..}=>"profile", Self::SetFanMode{..}=>"fan_mode",Self::SetFanSpeeds{..}=>"fan_custom", Self::SetLighting{..}=>"lighting",Self::SetLightingAnimation{..}=>"lighting_dynamic",Self::RestoreLighting{}=>"lighting_restore",Self::SetBatteryCare{..}=>"battery_care" } } }
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub captured_at_ms: u64, pub cpu_temperature_c: Option<f64>, pub gpu_temperature_c: Option<f64>,
    pub cpu_fan_rpm: Option<f64>, pub gpu_fan_rpm: Option<f64>, pub cpu_fan_percent:Option<u8>, pub gpu_fan_percent:Option<u8>, pub profile: Option<Profile>, pub fan_mode: Option<FanMode>,
    pub lighting: Option<Lighting>, pub lighting_effect:Option<LightingEffect>,pub lighting_config:Option<LightingConfig>,pub battery_care: Option<bool>, pub ac_online: Option<bool>, pub battery_percent: Option<u8>, pub reads: Vec<RawRead>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Capability { pub id: String, pub readable: bool, pub writable: bool, pub status: CapabilityStatus, pub reason: String }
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all="snake_case")]
pub enum CapabilityStatus { Verified, ImplementedUnverified, ProtocolPending, PermissionBlocked, HardwareUnsupported }
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Capabilities { pub model: String, pub bios: String, pub matched_model: bool, pub items: Vec<Capability> }
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all="snake_case")]
pub enum ApplyStatus { Applied, Failed, Unconfirmed, Rejected }
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplyReport { pub status: ApplyStatus, pub message: String, pub before: Option<Snapshot>, pub after: Option<Snapshot>, pub writes: Vec<RawRead> }

/// Narrow acknowledgement proved by the 2026-11-01 PHN16-71/V1.18 run.
/// Missing ReturnValue is accepted only for this exact setter/request pair;
/// gmOutput must be exactly zero, not merely have a zero low byte.
pub fn verified_fan_ack(write:&RawRead,mode:FanMode)->Option<bool>{
    if write.error.is_some(){return Some(false);}
    if write.class!="AcerGamingFunction"||write.method!="SetGamingFanBehavior"{return None;}
    let word=match mode{FanMode::Auto=>0x410009,FanMode::Max=>0x820009,FanMode::Custom=>return None};
    if write.input.len()!=1||write.input.get("gmInput").and_then(RawValue::as_u64)!=Some(word){return Some(false);}
    match write.output.get("ReturnValue"){None|Some(RawValue::Bool(true))=>{},Some(_)=>return Some(false)}
    write.output.get("gmOutput")?.as_u64().map(|status|status==0)
}
pub fn verified_fan_readback(snapshot:&Snapshot,mode:FanMode)->bool{
    if snapshot.fan_mode!=Some(mode){return false;}
    let behavior=|selector:u64|snapshot.reads.iter().find(|r|r.class=="AcerGamingFunction"&&r.method=="GetGamingFanBehavior"&&r.input.get("gmInput").and_then(RawValue::as_u64)==Some(selector)).and_then(RawRead::scalar);
    let Some(cpu)=behavior(1) else{return false;};let Some(gpu)=behavior(8) else{return false;};let Some(both)=behavior(9) else{return false;};
    fan_mode(cpu,gpu)==Some(mode)&&fan_mode(both,both)==Some(mode)
}
pub fn fan_control_ready(snapshot:&Snapshot)->bool{snapshot.cpu_fan_rpm.is_some()&&snapshot.gpu_fan_rpm.is_some()&&snapshot.fan_mode.is_some_and(|mode|verified_fan_readback(snapshot,mode))}
/// Controlled Auto/Max restore experiments reject an original Custom state.
pub fn verified_fan_ready(snapshot:&Snapshot)->bool{matches!(snapshot.fan_mode,Some(FanMode::Auto|FanMode::Max))&&fan_control_ready(snapshot)}
/// Local official SUPPORT_FAN_CONTROL IPC reports 0..100 for CPU and GPU.
/// Only the 70% physical point has been tested; every other request needs readback.
pub const CUSTOM_MIN_PERCENT:u8=0;
pub const CUSTOM_MAX_PERCENT:u8=100;
pub fn custom_percent_allowed(cpu:u8,gpu:u8)->bool{cpu<=CUSTOM_MAX_PERCENT&&gpu<=CUSTOM_MAX_PERCENT}
pub fn verified_custom_ready(snapshot:&Snapshot,cpu:u8,gpu:u8)->bool{custom_percent_allowed(cpu,gpu)&&fan_control_ready(snapshot)&&snapshot.cpu_fan_percent.is_some()&&snapshot.gpu_fan_percent.is_some()}
pub fn fan_percent(raw:u64)->Option<u8>{if raw&255!=0{return None;}let value=((raw>>8)&255)as u8;(value<=100).then_some(value)}
pub fn profile_power_ready(snapshot:&Snapshot)->bool{snapshot.ac_online==Some(true)&&snapshot.battery_percent.is_some_and(|n|(40..=100).contains(&n))}
pub fn profile_supported(snapshot:&Snapshot,profile:Profile)->bool{snapshot.reads.iter().find(|r|r.class=="AcerGamingFunction"&&r.method=="GetGamingMiscSetting"&&r.input.get("gmInput").and_then(RawValue::as_u64)==Some(10)).and_then(RawRead::scalar).is_some_and(|mask|((mask>>8)&(1u64<<profile.code()))!=0)}
pub fn profile_trial_ready(snapshot:&Snapshot,target:Profile)->bool{matches!(target,Profile::Balanced|Profile::Quiet|Profile::Turbo)&&profile_power_ready(snapshot)&&snapshot.profile.is_some_and(|p|p!=Profile::Eco&&profile_supported(snapshot,p))&&profile_supported(snapshot,target)}
pub fn profile_validation_ready(snapshot:&Snapshot)->bool{profile_trial_ready(snapshot,Profile::Balanced)}
pub fn verified_profile_ready(snapshot:&Snapshot,target:Profile)->bool{matches!(target,Profile::Quiet|Profile::Balanced|Profile::Performance|Profile::Turbo)&&profile_power_ready(snapshot)&&snapshot.profile.is_some_and(|p|p!=Profile::Eco&&profile_supported(snapshot,p))&&profile_supported(snapshot,target)}
pub fn verified_profile_ack(write:&RawRead,profile:Profile)->Option<bool>{
    if !matches!(profile,Profile::Quiet|Profile::Balanced|Profile::Performance|Profile::Turbo){return None;}
    if write.error.is_some(){return Some(false);}
    if write.class!="AcerGamingFunction"||write.method!="SetGamingMiscSetting"{return None;}
    if write.input.len()!=1||write.input.get("gmInput").and_then(RawValue::as_u64)!=Some(11|((profile.code()as u64)<<8)){return Some(false);}
    match write.output.get("ReturnValue"){None|Some(RawValue::Bool(true))=>{},_=>return Some(false)}
    write.output.get("gmOutput")?.as_u64().map(|status|status==0)
}
pub fn verified_profile_result(profile:Profile,writes:&[RawRead],after:Option<&Snapshot>)->ApplyStatus{
    if writes.len()!=1{return ApplyStatus::Unconfirmed;}
    match verified_profile_ack(&writes[0],profile){Some(false)=>ApplyStatus::Failed,Some(true)if after.is_some_and(|s|verified_profile_ready(s,profile)&&s.profile==Some(profile)&&s.reads.iter().find(|r|r.class=="AcerGamingFunction"&&r.method=="GetGamingMiscSetting"&&r.input.get("gmInput").and_then(RawValue::as_u64)==Some(11)).and_then(RawRead::scalar).and_then(Profile::decode)==Some(profile))=>ApplyStatus::Applied,_=>ApplyStatus::Unconfirmed}
}
/// Provisional acknowledgement for the fixed supervised custom-fan plan only.
/// This does not add either setter to the Verified policy.
pub fn supervised_custom_ack(call:&Call,read:&RawRead)->Option<bool>{
    if read.error.is_some(){return Some(false);}
    if call.class!="AcerGamingFunction"||!matches!(call.method.as_str(),"SetGamingFanBehavior"|"SetGamingFanSpeed")||read.class!=call.class||read.method!=call.method||read.input!=call.input{return Some(false);}
    match read.output.get("ReturnValue"){None|Some(RawValue::Bool(true))=>{},_=>return Some(false)}
    read.output.get("gmOutput")?.as_u64().map(|status|status==0)
}
pub fn custom_fan_readback(snapshot:&Snapshot,cpu_percent:u8,gpu_percent:u8)->bool{
    if cpu_percent>100||gpu_percent>100||snapshot.fan_mode!=Some(FanMode::Custom){return false;}
    let read=|method:&str,selector:u64|snapshot.reads.iter().find(|r|r.class=="AcerGamingFunction"&&r.method==method&&r.input.get("gmInput").and_then(RawValue::as_u64)==Some(selector)).and_then(RawRead::scalar);
    let(Some(cpu),Some(gpu),Some(both),Some(cpu_speed),Some(gpu_speed))=(read("GetGamingFanBehavior",1),read("GetGamingFanBehavior",8),read("GetGamingFanBehavior",9),read("GetGamingFanSpeed",1),read("GetGamingFanSpeed",4))else{return false;};
    (cpu>>8)&3==3&&(gpu>>14)&3==3&&(both>>8)&3==3&&(both>>14)&3==3&&((cpu_speed>>8)&255)==cpu_percent as u64&&((gpu_speed>>8)&255)==gpu_percent as u64
}
pub fn verified_custom_result(cpu:u8,gpu:u8,writes:&[RawRead],after:Option<&Snapshot>)->ApplyStatus{
    if !custom_percent_allowed(cpu,gpu){return ApplyStatus::Rejected;}
    let calls=encode(&Command::SetFanSpeeds{cpu_percent:cpu,gpu_percent:gpu}).expect("range checked");
    if calls.iter().zip(writes).any(|(c,r)|supervised_custom_ack(c,r)==Some(false)){return ApplyStatus::Failed;}
    if writes.len()!=3||!calls.iter().zip(writes).all(|(c,r)|supervised_custom_ack(c,r)==Some(true)){return ApplyStatus::Unconfirmed;}
    if after.is_some_and(|s|s.cpu_fan_percent==Some(cpu)&&s.gpu_fan_percent==Some(gpu)&&custom_fan_readback(s,cpu,gpu)){ApplyStatus::Applied}else{ApplyStatus::Unconfirmed}
}
pub fn verified_fan_result(mode:FanMode,writes:&[RawRead],after:Option<&Snapshot>)->ApplyStatus{
    if writes.len()!=1{return ApplyStatus::Unconfirmed;}
    match verified_fan_ack(&writes[0],mode){Some(false)=>ApplyStatus::Failed,Some(true) if after.is_some_and(|s|verified_fan_readback(s,mode))=>ApplyStatus::Applied,_=>ApplyStatus::Unconfirmed}
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Call { pub class: String, pub method: String, pub input: BTreeMap<String, RawValue> }
pub fn gaming(method: &str, value: u64) -> Call { Call { class:"AcerGamingFunction".into(),method:method.into(),input:BTreeMap::from([("gmInput".into(),RawValue::unsigned(value))]) } }
pub mod calibration;
pub fn battery_read() -> Call { Call { class:"BatteryControl".into(),method:"GetBatteryHealthControlStatus".into(),input:BTreeMap::from([("uBatteryNo".into(),RawValue::unsigned(1)),("uFunctionQuery".into(),RawValue::unsigned(1)),("uReserved".into(),RawValue::Bytes(vec![0,0]))]) } }
pub fn encode(command: &Command) -> Result<Vec<Call>, DeviceError> {
    Ok(match command {
        Command::SetProfile{profile}=>vec![gaming("SetGamingMiscSetting",11 | ((profile.code() as u64)<<8))],
        Command::SetFanMode{mode}=>vec![gaming("SetGamingFanBehavior",match mode {FanMode::Auto=>0x410009,FanMode::Max=>0x820009,FanMode::Custom=>return Err(DeviceError{kind:ErrorKind::InvalidArgument,message:"Custom requires explicit CPU/GPU percentages via SetFanSpeeds".into()})})],
        Command::SetFanSpeeds{cpu_percent,gpu_percent}=>{
            if *cpu_percent>100||*gpu_percent>100 {return Err(DeviceError{kind:ErrorKind::InvalidArgument,message:"Fan percentages must be 0..=100".into()});}
            vec![gaming("SetGamingFanBehavior",0xc30009),gaming("SetGamingFanSpeed",((*cpu_percent as u64)<<8)|1),gaming("SetGamingFanSpeed",((*gpu_percent as u64)<<8)|4)]
        },
        Command::SetLighting{lighting}=>{
            if lighting.brightness>100{return Err(DeviceError{kind:ErrorKind::InvalidArgument,message:"Keyboard brightness must be 0..=100".into()});}
            let mut calls=Vec::new();for (index,color) in lighting.zones.iter().enumerate(){calls.push(gaming("SetGamingRgbKb",(1<<index)|((color[0] as u64)<<8)|((color[1] as u64)<<16)|((color[2] as u64)<<24)));}
            // Candidate official PredatorSense static layout. Dynamic layout conflicts are not guessed here.
            calls.push(Call{class:"AcerGamingFunction".into(),method:"SetGamingKBBacklight".into(),input:BTreeMap::from([("gmInput".into(),RawValue::Bytes(backlight_payload(0,0,lighting.brightness,0,[0;3])?))])});calls
        },
        Command::SetLightingAnimation{lighting}=>vec![backlight_frame_call(animation_frame(lighting)?)],
        Command::RestoreLighting{}=>return Err(DeviceError{kind:ErrorKind::InvalidArgument,message:"Lighting restore requires the worker-held session receipt; no caller-supplied frame is accepted".into()}),
        Command::SetBatteryCare{enabled}=>vec![Call {class:"BatteryControl".into(),method:"SetBatteryHealthControl".into(),input:BTreeMap::from([("uBatteryNo".into(),RawValue::unsigned(1)),("uFunctionMask".into(),RawValue::unsigned(1)),("uFunctionStatus".into(),RawValue::unsigned(u64::from(*enabled))),("uReservedIn".into(),RawValue::Bytes(vec![0;5]))])}],
    })
}
pub fn battery_care(read: &RawRead) -> Option<bool> {
    battery_health_state(read).map(|state|state.enabled)
}
/// Reference scaling; reject fractional and implausible/sentinel values rather than showing them as RPM.
pub fn fan_rpm(raw: u64) -> Option<f64> { sensor_value(raw).filter(|n|*n<=20_000).map(f64::from) }
/// Explicit official PredatorSense candidate layout. Callers must resolve the local
/// controller/layout conflict before enabling this for actual dynamic writes.
pub fn backlight_payload(effect:u8,speed:u8,brightness:u8,direction:u8,color:[u8;3])->Result<Vec<u8>,DeviceError>{
    if effect>7||brightness>100||(effect!=0&&!(1..=9).contains(&speed))||(effect!=0&&!(1..=2).contains(&direction)){return Err(DeviceError{kind:ErrorKind::InvalidArgument,message:"Invalid effect/speed/brightness/direction".into()});}
    let mut b=vec![0;16];b[0]=effect;b[1]=if effect==0{0}else{speed};b[2]=brightness;b[4]=if effect==0{0}else{direction};
    if !matches!(effect,0|2|3){b[5..8].copy_from_slice(&color);}b[8]=3;b[9]=1;Ok(b)
}
pub fn static_lighting(backlight:&RawRead,zones:&[RawRead])->Option<Lighting>{
    if !backlight.transport_ok()||backlight.output.get("gmReturn")?.as_u64()!=Some(0)||zones.len()!=4{return None;}
    let RawValue::Bytes(bytes)=backlight.output.get("gmOutput")? else{return None;};
    if bytes.len()<9||bytes[0]!=0||bytes[2]>100||bytes[8]!=3{return None;}
    let mut colors=[[0;3];4];for (index,zone) in zones.iter().enumerate(){let raw=zone.scalar()?;colors[index]=[((raw>>8)&255)as u8,((raw>>16)&255)as u8,((raw>>24)&255)as u8];}
    Some(Lighting{brightness:bytes[2],zones:colors})
}
#[cfg(test)] mod tests {
    use super::*;
    #[test] fn profile_roundtrip_and_unknown() { for p in [Profile::Quiet,Profile::Balanced,Profile::Performance,Profile::Turbo,Profile::Eco] { let c=encode(&Command::SetProfile{profile:p}).unwrap(); assert_eq!(Profile::decode(c[0].input["gmInput"].as_u64().unwrap()),Some(p)); } assert_eq!(Profile::decode(0x300),None); }
    #[test] fn fan_words_match_fixed_reference() { assert_eq!(encode(&Command::SetFanMode{mode:FanMode::Auto}).unwrap()[0].input["gmInput"].as_u64(),Some(0x410009)); assert_eq!(encode(&Command::SetFanMode{mode:FanMode::Max}).unwrap()[0].input["gmInput"].as_u64(),Some(0x820009)); }
    #[test] fn raw_u64_survives_javascript_boundary() { let v=RawValue::unsigned(u64::MAX); let json=serde_json::to_string(&v).unwrap(); assert!(json.contains("\"18446744073709551615\"")); assert_eq!(serde_json::from_str::<RawValue>(&json).unwrap(),v); }
    #[test] fn boolean_return_is_strict() { let mut r=RawRead{class:"".into(),method:"".into(),input:BTreeMap::new(),output:BTreeMap::from([("ReturnValue".into(),RawValue::unsigned(0)),("gmOutput".into(),RawValue::unsigned(256))]),error:None}; assert_eq!(r.scalar(),None); r.output.insert("ReturnValue".into(),RawValue::Bool(true)); assert_eq!(r.scalar(),Some(256)); r.output.insert("gmOutput".into(),RawValue::unsigned(257));assert_eq!(r.scalar(),None); }
    #[test] fn battery_requires_both_statuses() { let mut r=RawRead{class:"BatteryControl".into(),method:"GetBatteryHealthControlStatus".into(),input:BTreeMap::new(),output:BTreeMap::from([("ReturnValue".into(),RawValue::Bool(true)),("uFunctionList".into(),RawValue::unsigned(3)),("uReturn".into(),RawValue::Bytes(vec![0,0])),("uFunctionStatus".into(),RawValue::Bytes(vec![1,0,0,0,0]))]),error:None}; assert_eq!(battery_care(&r),Some(true)); r.output.insert("uReturn".into(),RawValue::Bytes(vec![1,0])); assert_eq!(battery_care(&r),None); }
    #[test] fn rpm_sentinels_stay_unknown() { assert_eq!(fan_rpm(3500*256),Some(3500.0)); assert_eq!(fan_rpm(0),Some(0.0)); assert_eq!(fan_rpm(u64::MAX),None); assert_eq!(fan_rpm(3500*256+1),None); assert_eq!(Profile::decode(0x1000000000100),None); }
    #[test] fn independent_fan_percentages_are_checked_and_ordered() {
        let plan=encode(&Command::SetFanSpeeds{cpu_percent:40,gpu_percent:65}).unwrap();
        assert_eq!(plan.len(),3);assert_eq!(plan[0].input["gmInput"].as_u64(),Some(0xc30009));assert_eq!(plan[1].input["gmInput"].as_u64(),Some(0x2801));assert_eq!(plan[2].input["gmInput"].as_u64(),Some(0x4104));
        assert_eq!(encode(&Command::SetFanSpeeds{cpu_percent:101,gpu_percent:50}).unwrap_err().kind,ErrorKind::InvalidArgument);
    }
    #[test] fn combined_fan_mode_recognizes_custom_and_keeps_mixed_unknown() { assert_eq!(fan_mode(0x4100,0x4100),Some(FanMode::Auto));assert_eq!(fan_mode(0x8200,0x8200),Some(FanMode::Max));assert_eq!(fan_mode(0x100,0x8000),None);assert_eq!(fan_mode(0xc300,0xc300),Some(FanMode::Custom));assert_eq!(fan_mode(2,2),None); }
    #[test] fn sensors_require_mask_status_and_real_temperature() {let mask=(1u64<<24)|(1u64<<33);assert!(sensor_supported(mask,1));assert!(sensor_supported(mask,10));assert!(!sensor_supported(mask,2));assert!(!sensor_supported(mask|1,1));assert_eq!(temperature(73<<8),Some(73.0));assert_eq!(temperature(0),None);assert_eq!(temperature((73<<8)|2),None);}
    #[test] fn static_rgb_has_reviewed_byte_order_and_length(){let lighting=Lighting{brightness:75,zones:[[0x11,0x22,0x33];4]};let calls=encode(&Command::SetLighting{lighting}).unwrap();assert_eq!(calls[0].input["gmInput"].as_u64(),Some(0x33221101));assert_eq!(calls[3].input["gmInput"].as_u64(),Some(0x33221108));assert_eq!(calls[4].input["gmInput"],RawValue::Bytes(vec![0,0,75,0,0,0,0,0,3,1,0,0,0,0,0,0]));assert!(backlight_payload(3,10,100,1,[0;3]).is_err());}
    #[test] fn firmware_failure_is_distinct_from_missing_status(){let mut r=RawRead{class:"AcerGamingFunction".into(),method:"SetGamingFanSpeed".into(),input:BTreeMap::new(),output:BTreeMap::from([("ReturnValue".into(),RawValue::Bool(true))]),error:None};assert_eq!(r.firmware_write_ok(),None);r.output.insert("gmOutput".into(),RawValue::unsigned(2));assert_eq!(r.firmware_write_ok(),Some(false));r.output.insert("gmOutput".into(),RawValue::unsigned(0));assert_eq!(r.firmware_write_ok(),Some(true));r.output.remove("ReturnValue");assert_eq!(r.firmware_write_ok(),None);r.output.insert("gmOutput".into(),RawValue::unsigned(2));assert_eq!(r.firmware_write_ok(),Some(false));}
    #[test] fn actual_phn16_71_getter_outputs_omit_return_value(){
        let sample:Snapshot=serde_json::from_str(include_str!("../tests/fixtures/phn16-71-elevated-read.json")).unwrap();
        let reads=&sample.reads;assert!(!reads[0].output.contains_key("ReturnValue"));
        assert_eq!(reads[0].scalar().and_then(fan_rpm),Some(3829.0));assert_eq!(reads[1].scalar().and_then(fan_rpm),Some(3750.0));
        assert_eq!(reads[2].scalar().and_then(Profile::decode),Some(Profile::Performance));
        assert_eq!(fan_mode(reads[3].scalar().unwrap(),reads[4].scalar().unwrap()),Some(FanMode::Auto));
        let lighting=static_lighting(&reads[5],&reads[6..10]).unwrap();assert_eq!(lighting.brightness,0);assert_eq!(lighting.zones,[[0,174,199];4]);
        let battery=reads.iter().find(|r|r.method=="GetBatteryHealthControlStatus").unwrap();assert_eq!(battery_care(battery),Some(false));
        let mask=reads.iter().find(|r|r.method=="GetGamingSysInfo"&&r.input["gmInput"].as_u64()==Some(0)).unwrap().scalar().unwrap();assert!(sensor_supported(mask,1));assert!(sensor_supported(mask,10));
        let mut explicit_failure=reads[0].clone();explicit_failure.output.insert("ReturnValue".into(),RawValue::Bool(false));assert_eq!(explicit_failure.scalar(),None);
        let mut wrong_method=reads[0].clone();wrong_method.method="SetGamingFanSpeed".into();assert!(!wrong_method.transport_ok());assert_eq!(wrong_method.firmware_write_ok(),None);
    }
    fn real_fan_reports()->(ApplyReport,ApplyReport){let value:serde_json::Value=serde_json::from_str(include_str!("../tests/fixtures/phn16-71-fan-max-restore.json")).unwrap();(serde_json::from_value(value["max_apply"].clone()).unwrap(),serde_json::from_value(value["restore_apply"].clone()).unwrap())}
    #[test] fn verified_fan_accepts_only_proven_ack_and_three_matching_reads(){
        let(max,restore)=real_fan_reports();
        assert!(!max.writes[0].output.contains_key("ReturnValue"));assert_eq!(verified_fan_ack(&max.writes[0],FanMode::Max),Some(true));
        assert!(verified_fan_ready(max.before.as_ref().unwrap()));assert_eq!(verified_fan_result(FanMode::Max,&max.writes,max.after.as_ref()),ApplyStatus::Applied);
        assert_eq!(verified_fan_result(FanMode::Auto,&restore.writes,restore.after.as_ref()),ApplyStatus::Applied);
    }
    #[test] fn verified_fan_rejects_errors_and_keeps_partial_readback_unconfirmed(){
        let(mut max,_)=real_fan_reports();let after=max.after.clone().unwrap();
        max.writes[0].output.insert("ReturnValue".into(),RawValue::Bool(false));assert_eq!(verified_fan_result(FanMode::Max,&max.writes,Some(&after)),ApplyStatus::Failed);
        max.writes[0].output.remove("ReturnValue");max.writes[0].output.insert("gmOutput".into(),RawValue::unsigned(256));assert_eq!(verified_fan_result(FanMode::Max,&max.writes,Some(&after)),ApplyStatus::Failed);
        max.writes[0].output.clear();assert_eq!(verified_fan_result(FanMode::Max,&max.writes,Some(&after)),ApplyStatus::Unconfirmed);
        let(max,_)=real_fan_reports();let mut partial=max.after.clone().unwrap();partial.reads.retain(|r|!(r.method=="GetGamingFanBehavior"&&r.input.get("gmInput").and_then(RawValue::as_u64)==Some(9)));assert_eq!(verified_fan_result(FanMode::Max,&max.writes,Some(&partial)),ApplyStatus::Unconfirmed);
        let mut wrong=max.after.clone().unwrap();let gpu=wrong.reads.iter_mut().find(|r|r.method=="GetGamingFanBehavior"&&r.input.get("gmInput").and_then(RawValue::as_u64)==Some(8)).unwrap();gpu.output.insert("gmOutput".into(),RawValue::unsigned(0x4000));assert_eq!(verified_fan_result(FanMode::Max,&max.writes,Some(&wrong)),ApplyStatus::Unconfirmed);
        assert_eq!(verified_fan_result(FanMode::Max,&max.writes,None),ApplyStatus::Unconfirmed);
    }
    #[test]fn profile_trial_needs_official_power_threshold_and_supported_restorable_mode(){let mut s:Snapshot=serde_json::from_str(include_str!("../tests/fixtures/phn16-71-elevated-read.json")).unwrap();s.profile=Some(Profile::Performance);s.battery_percent=Some(40);assert!(profile_validation_ready(&s));s.battery_percent=Some(39);assert!(!profile_validation_ready(&s));s.battery_percent=Some(100);s.profile=Some(Profile::Eco);assert!(!profile_validation_ready(&s));s.profile=None;assert!(!profile_validation_ready(&s));s.profile=Some(Profile::Performance);s.reads.retain(|r|!(r.method=="GetGamingMiscSetting"&&r.input.get("gmInput").and_then(RawValue::as_u64)==Some(10)));assert!(!profile_validation_ready(&s));}
    fn real_profile_reports()->(ApplyReport,ApplyReport){let value:serde_json::Value=serde_json::from_str(include_str!("../tests/fixtures/phn16-71-profile-balanced-restore.json")).unwrap();(serde_json::from_value(value["trial_apply"].clone()).unwrap(),serde_json::from_value(value["restore_apply"].clone()).unwrap())}
    #[test]fn verified_profile_uses_real_acks_and_profile_guard_independent_of_fans(){let(balanced,restore)=real_profile_reports();assert!(!balanced.writes[0].output.contains_key("ReturnValue"));assert_eq!(verified_profile_result(Profile::Balanced,&balanced.writes,balanced.after.as_ref()),ApplyStatus::Applied);assert_eq!(verified_profile_result(Profile::Performance,&restore.writes,restore.after.as_ref()),ApplyStatus::Applied);let mut no_fans=balanced.after.clone().unwrap();no_fans.fan_mode=None;no_fans.cpu_fan_rpm=None;no_fans.gpu_fan_rpm=None;no_fans.reads.retain(|r|!matches!(r.method.as_str(),"GetGamingFanBehavior"|"GetGamingFanSpeed"));assert!(!verified_fan_ready(&no_fans));assert!(verified_profile_ready(&no_fans,Profile::Balanced));assert_eq!(verified_profile_result(Profile::Balanced,&balanced.writes,Some(&no_fans)),ApplyStatus::Applied);}
    #[test]fn verified_profile_rejects_power_unknown_targets_ack_or_readback_mismatch(){let(mut balanced,_)=real_profile_reports();let after=balanced.after.clone().unwrap();assert_eq!(verified_profile_ack(&balanced.writes[0],Profile::Eco),None);let mut no_ac=after.clone();no_ac.ac_online=Some(false);assert!(!verified_profile_ready(&no_ac,Profile::Balanced));assert_eq!(verified_profile_result(Profile::Balanced,&balanced.writes,Some(&no_ac)),ApplyStatus::Unconfirmed);balanced.writes[0].output.insert("gmOutput".into(),RawValue::unsigned(1));assert_eq!(verified_profile_result(Profile::Balanced,&balanced.writes,Some(&after)),ApplyStatus::Failed);balanced.writes[0].output.insert("gmOutput".into(),RawValue::unsigned(0));let mut wrong=after.clone();wrong.profile=Some(Profile::Performance);assert_eq!(verified_profile_result(Profile::Balanced,&balanced.writes,Some(&wrong)),ApplyStatus::Unconfirmed);}
    fn decode_old_fan_snapshot(mut snapshot:Snapshot)->Snapshot{let read=|method:&str,id:u64|snapshot.reads.iter().find(|r|r.method==method&&r.input.get("gmInput").and_then(RawValue::as_u64)==Some(id)).and_then(RawRead::scalar);snapshot.fan_mode=read("GetGamingFanBehavior",1).zip(read("GetGamingFanBehavior",8)).and_then(|(c,g)|fan_mode(c,g));snapshot.cpu_fan_percent=read("GetGamingFanSpeed",1).and_then(fan_percent);snapshot.gpu_fan_percent=read("GetGamingFanSpeed",4).and_then(fan_percent);snapshot}
    #[test]fn actual_custom70_decodes_custom_percentages_and_can_return_to_auto(){let value:serde_json::Value=serde_json::from_str(include_str!("../tests/fixtures/phn16-71-fan-custom-restore.json")).unwrap();let trial:ApplyReport=serde_json::from_value(value["trial_apply"].clone()).unwrap();let after=decode_old_fan_snapshot(trial.after.clone().unwrap());assert_eq!(after.fan_mode,Some(FanMode::Custom));assert_eq!((after.cpu_fan_percent,after.gpu_fan_percent),(Some(70),Some(70)));assert!(fan_control_ready(&after));assert!(!verified_fan_ready(&after));assert_eq!(verified_custom_result(70,70,&trial.writes,Some(&after)),ApplyStatus::Applied);let mut wrong=after.clone();wrong.gpu_fan_percent=Some(69);assert_eq!(verified_custom_result(70,70,&trial.writes,Some(&wrong)),ApplyStatus::Unconfirmed);let restore:ApplyReport=serde_json::from_value(value["restore_apply"].clone()).unwrap();assert_eq!(verified_fan_result(FanMode::Auto,&restore.writes,restore.after.as_ref()),ApplyStatus::Applied);assert_eq!(verified_custom_result(70,70,&trial.writes[..2],Some(&after)),ApplyStatus::Unconfirmed);}
    #[test]fn official_custom_range_has_distinct_custom_commands_at_zero_and_max(){for pct in [0,70,100]{assert!(custom_percent_allowed(pct,pct));let calls=encode(&Command::SetFanSpeeds{cpu_percent:pct,gpu_percent:pct}).unwrap();assert_eq!(calls.len(),3);assert_eq!(calls[0].input["gmInput"].as_u64(),Some(0xc30009));}assert!(!custom_percent_allowed(101,70));assert!(encode(&Command::SetFanMode{mode:FanMode::Custom}).is_err());assert_eq!(fan_percent(101<<8),None);}
    #[test]fn actual_quiet_and_turbo_acks_have_matching_target_readback(){for(profile,json)in[(Profile::Quiet,include_str!("../tests/fixtures/phn16-71-profile-quiet-restore.json")),(Profile::Turbo,include_str!("../tests/fixtures/phn16-71-profile-turbo-restore.json"))]{let value:serde_json::Value=serde_json::from_str(json).unwrap();let trial:ApplyReport=serde_json::from_value(value["trial_apply"].clone()).unwrap();assert_eq!(verified_profile_result(profile,&trial.writes,trial.after.as_ref()),ApplyStatus::Applied);}}
    #[test]fn actual_getter15_reconstructs_setter16_and_brightness_is_only_state_change(){let snapshot:Snapshot=serde_json::from_str(include_str!("../tests/fixtures/phn16-71-elevated-read.json")).unwrap();let saved=observed_backlight_frame(&snapshot).unwrap();assert_eq!((saved.observed_state.len(),saved.setter_frame.len()),(15,16));assert_eq!(saved.observed_state[9],0);assert_eq!(saved.setter_frame[9],1);assert_eq!(saved.setter_frame[15],0);let target=brightness_frame(&saved,20).unwrap();let changed:Vec<_>=saved.setter_frame.iter().zip(&target).enumerate().filter_map(|(i,(a,b))|(a!=b).then_some(i)).collect();assert_eq!(changed,[2]);let call=backlight_frame_call(target.clone());let mut ack=RawRead{class:call.class,method:call.method,input:call.input,output:BTreeMap::from([("gmOutput".into(),RawValue::unsigned(0))]),error:None};assert_eq!(backlight_frame_ack(&ack,&target),Some(true));ack.output.insert("ReturnValue".into(),RawValue::Bool(false));assert_eq!(backlight_frame_ack(&ack,&target),Some(false));}
    #[test]fn lighting_comparison_normalizes_only_device_routing_and_checks_remaining_state(){let snapshot:Snapshot=serde_json::from_str(include_str!("../tests/fixtures/phn16-71-elevated-read.json")).unwrap();let saved=observed_backlight_frame(&snapshot).unwrap();let mut after=snapshot.clone();let state=after.reads.iter_mut().find(|r|r.method=="GetGamingKBBacklight").unwrap().output.get_mut("gmOutput").unwrap();let RawValue::Bytes(bytes)=state else{panic!()};bytes[2]=20;bytes[9]=1;assert!(backlight_frame_matches(&after,&saved,20));let read=after.reads.iter_mut().find(|r|r.method=="GetGamingKBBacklight").unwrap();if let Some(RawValue::Bytes(bytes))=read.output.get_mut("gmOutput"){bytes[6]^=1;}assert!(!backlight_frame_matches(&after,&saved,20));let mut malformed=snapshot.clone();malformed.reads.iter_mut().find(|r|r.method=="GetGamingKBBacklight").unwrap().output.insert("gmReturn".into(),RawValue::unsigned(1));assert!(observed_backlight_frame(&malformed).is_none());}
    #[test]fn static_palette_and_original_restore_plan_have_distinct_expected_words(){let snapshot:Snapshot=serde_json::from_str(include_str!("../tests/fixtures/phn16-71-elevated-read.json")).unwrap();let saved=observed_backlight_frame(&snapshot).unwrap();let calls=static_color_trial_plan(&saved).unwrap();assert_eq!(calls.len(),5);assert_eq!(calls[1].input["gmInput"].as_u64(),Some(0x0000ff01));assert_eq!(calls[2].input["gmInput"].as_u64(),Some(0x00ff0002));assert_eq!(calls[3].input["gmInput"].as_u64(),Some(0xff000004));assert_eq!(calls[4].input["gmInput"].as_u64(),Some(0xffffff08));let restore=static_color_restore_plan(&saved).unwrap();for(i,call)in restore[..4].iter().enumerate(){assert_eq!(call.input["gmInput"].as_u64(),Some(0xc7ae0000|(1u64<<i)));}assert_eq!(restore[4].input["gmInput"],RawValue::Bytes(saved.setter_frame));}
    #[test]fn battery_health_trial_excludes_calibration_and_uses_uint16_return(){let mut snapshot:Snapshot=serde_json::from_str(include_str!("../tests/fixtures/phn16-71-elevated-read.json")).unwrap();snapshot.battery_care=Some(false);let saved=battery_validation_state(&snapshot).unwrap();assert_eq!(saved.function_list,3);assert!(!saved.calibrating);let call=encode(&Command::SetBatteryCare{enabled:true}).unwrap().remove(0);assert_eq!(call.input["uFunctionMask"].as_u64(),Some(1));let mut ack=RawRead{class:call.class,method:call.method,input:call.input,output:BTreeMap::from([("uReturn".into(),RawValue::unsigned(0))]),error:None};assert_eq!(battery_care_ack(&ack,true),Some(true));assert_eq!(battery_care_ack(&ack,false),Some(false));ack.output.insert("uReturn".into(),RawValue::Bytes(vec![0,0]));assert_eq!(battery_care_ack(&ack,true),None);let r=snapshot.reads.iter_mut().find(|r|r.method=="GetBatteryHealthControlStatus").unwrap();if let Some(RawValue::Bytes(b))=r.output.get_mut("uFunctionStatus"){b[1]=1;}assert!(battery_validation_state(&snapshot).is_none());}
    #[test]fn actual_static_scene_is_verified_with_per_step_ack_and_independent_guard(){let value:serde_json::Value=serde_json::from_str(include_str!("../tests/fixtures/phn16-71-lighting-static-colors-restore.json")).unwrap();let mut trial:ApplyReport=serde_json::from_value(value["trial_apply"].clone()).unwrap();let saved=observed_backlight_frame(trial.before.as_ref().unwrap()).unwrap();let target=Lighting{brightness:20,zones:VALIDATION_STATIC_ZONES};assert_eq!(verified_static_result(&target,&saved,&trial.writes,trial.after.as_ref()),ApplyStatus::Applied);let mut no_fans=trial.before.clone().unwrap();no_fans.fan_mode=None;no_fans.cpu_fan_rpm=None;no_fans.gpu_fan_rpm=None;assert!(verified_static_ready(&no_fans,&target));trial.writes[2].output.insert("gmOutput".into(),RawValue::unsigned(1));assert_eq!(verified_static_result(&target,&saved,&trial.writes,trial.after.as_ref()),ApplyStatus::Failed);assert_eq!(verified_static_result(&target,&saved,&trial.writes[..1],trial.after.as_ref()),ApplyStatus::Unconfirmed);}
    #[test]fn actual_battery_bool_and_restore_use_verified_uint_ack_and_unchanged_functions(){let value:serde_json::Value=serde_json::from_str(include_str!("../tests/fixtures/phn16-71-battery-care-restore.json")).unwrap();let mut trial:ApplyReport=serde_json::from_value(value["trial_apply"].clone()).unwrap();let saved=battery_validation_state(trial.before.as_ref().unwrap()).unwrap();assert_eq!(verified_battery_result(true,&saved,&trial.writes,trial.after.as_ref()),ApplyStatus::Applied);let restore:ApplyReport=serde_json::from_value(value["restore_apply"].clone()).unwrap();let restore_before=battery_validation_state(restore.before.as_ref().unwrap()).unwrap();assert_eq!(verified_battery_result(false,&restore_before,&restore.writes,restore.after.as_ref()),ApplyStatus::Applied);trial.writes[0].output.insert("uReturn".into(),RawValue::unsigned(1));assert_eq!(verified_battery_result(true,&saved,&trial.writes,trial.after.as_ref()),ApplyStatus::Failed);}
    #[test]fn lighting_effects_decode_without_claiming_dynamic_write_validation(){for code in 0..8{let effect=LightingEffect::from_code(code).unwrap();assert_eq!(effect.code(),code);}assert_eq!(LightingEffect::from_code(8),None);let snapshot:Snapshot=serde_json::from_str(include_str!("../tests/fixtures/phn16-71-elevated-read.json")).unwrap();let read=snapshot.reads.iter().find(|r|r.method=="GetGamingKBBacklight").unwrap();assert_eq!(backlight_effect(read),Some(LightingEffect::Static));}
    #[test]fn dynamic_trial_checks_only_applicable_color_direction_and_restores_original_brightness(){let mut snapshot:Snapshot=serde_json::from_str(include_str!("../tests/fixtures/phn16-71-elevated-read.json")).unwrap();let read=snapshot.reads.iter_mut().find(|r|r.method=="GetGamingKBBacklight").unwrap();if let Some(RawValue::Bytes(state))=read.output.get_mut("gmOutput"){state[2]=75;}let saved=observed_backlight_frame(&snapshot).unwrap();let restore=dynamic_restore_plan(&saved).unwrap();assert_eq!(restore.len(),6);let RawValue::Bytes(stop)=&restore[0].input["gmInput"]else{panic!()};assert_eq!((stop[0],stop[2]),(0,0));let RawValue::Bytes(final_frame)=&restore[5].input["gmInput"]else{panic!()};assert_eq!((final_frame[0],final_frame[2]),(0,75));let frame=dynamic_trial_frame(LightingEffect::Wave).unwrap();let read=snapshot.reads.iter_mut().find(|r|r.method=="GetGamingKBBacklight").unwrap();read.output.insert("gmOutput".into(),RawValue::Bytes(frame[..15].to_vec()));snapshot.lighting_effect=Some(LightingEffect::Wave);assert!(dynamic_trial_matches(&snapshot,LightingEffect::Wave));let read=snapshot.reads.iter_mut().find(|r|r.method=="GetGamingKBBacklight").unwrap();if let Some(RawValue::Bytes(state))=read.output.get_mut("gmOutput"){state[5..8].copy_from_slice(&[1,2,3]);}assert!(dynamic_trial_matches(&snapshot,LightingEffect::Wave));let read=snapshot.reads.iter_mut().find(|r|r.method=="GetGamingKBBacklight").unwrap();if let Some(RawValue::Bytes(state))=read.output.get_mut("gmOutput"){state[4]=2;}assert!(!dynamic_trial_matches(&snapshot,LightingEffect::Wave));assert!(dynamic_trial_frame(LightingEffect::Static).is_none());}
    fn actual_seven_dynamic()->Vec<serde_json::Value>{serde_json::from_str(include_str!("../tests/fixtures/phn16-71-seven-dynamic-configs.json")).unwrap()}
    fn acked(calls:Vec<Call>)->Vec<RawRead>{calls.into_iter().map(|c|RawRead{class:c.class,method:c.method,input:c.input,output:BTreeMap::from([("gmOutput".into(),RawValue::unsigned(0))]),error:None}).collect()}
    #[test]fn real_seven_dynamic_points_support_formal_commands_and_static_receipt_restore(){for value in actual_seven_dynamic(){let code=value["code"].as_u64().unwrap()as u8;let effect=LightingEffect::from_code(code).unwrap();let before:Snapshot=serde_json::from_value(value["before"].clone()).unwrap();let after:Snapshot=serde_json::from_value(value["after"].clone()).unwrap();let restored:Snapshot=serde_json::from_value(value["restored"].clone()).unwrap();let writes:Vec<RawRead>=serde_json::from_value(value["writes"].clone()).unwrap();let restores:Vec<RawRead>=serde_json::from_value(value["restore_writes"].clone()).unwrap();let target=LightingAnimation{effect,speed:3,brightness:20,direction:1,color:DYNAMIC_TRIAL_COLOR};assert_eq!(verified_animation_result(&target,&writes,Some(&after)),ApplyStatus::Applied);let config=lighting_config_from_reads(&after.reads).unwrap();assert_eq!(config.effect,effect);assert_eq!(config.zones,None);assert!(lighting_control_ready(&after));let saved=observed_lighting_frame(&before).unwrap();assert_eq!(verified_lighting_restore_result(&saved,&restores,Some(&restored)),ApplyStatus::Applied);}}
    #[test]fn simulated_normal_transitions_and_dynamic_receipts_do_not_require_static_before(){let values=actual_seven_dynamic();let before:Snapshot=serde_json::from_value(values[0]["after"].clone()).unwrap();let wave:Snapshot=serde_json::from_value(values[2]["after"].clone()).unwrap();let saved=observed_lighting_frame(&before).unwrap();let target=LightingAnimation{effect:LightingEffect::Wave,speed:3,brightness:20,direction:1,color:DYNAMIC_TRIAL_COLOR};let writes=acked(encode(&Command::SetLightingAnimation{lighting:target.clone()}).unwrap());assert_eq!(verified_animation_result(&target,&writes,Some(&wave)),ApplyStatus::Applied);let static_target=Lighting{brightness:20,zones:VALIDATION_STATIC_ZONES};assert!(verified_static_ready(&before,&static_target));let calls=static_lighting_plan(&saved,&static_target).unwrap();let RawValue::Bytes(frame)=&calls[0].input["gmInput"]else{panic!()};assert_eq!((frame[0],frame[1],frame[4]),(0,0,0));let restore_calls=lighting_restore_plan(&saved).unwrap();let restores=acked(restore_calls);assert_eq!(verified_lighting_restore_result(&saved,&restores,Some(&before)),ApplyStatus::Applied);}
    #[test]fn missing_static_zones_are_unknown_and_not_filled_with_black(){let values=actual_seven_dynamic();let mut before:Snapshot=serde_json::from_value(values[0]["before"].clone()).unwrap();before.reads.retain(|r|r.method!="GetGamingRgbKb");assert_eq!(lighting_config_from_reads(&before.reads).unwrap().zones,None);assert!(!lighting_control_ready(&before));let mut dynamic:Snapshot=serde_json::from_value(values[0]["after"].clone()).unwrap();dynamic.reads.retain(|r|r.method!="GetGamingRgbKb");assert_eq!(lighting_config_from_reads(&dynamic.reads).unwrap().zones,None);assert!(lighting_control_ready(&dynamic));assert_eq!(lighting_restore_plan(&observed_lighting_frame(&dynamic).unwrap()).unwrap().len(),2);}
    #[test]fn typed_animation_rejects_invalid_ranges_and_restore_rejects_frontend_frames(){let mut target=LightingAnimation{effect:LightingEffect::Wave,speed:3,brightness:20,direction:1,color:[0;3]};assert!(valid_animation(&target));target.speed=0;assert!(!valid_animation(&target));target.speed=10;assert!(!valid_animation(&target));target.speed=9;target.direction=3;assert!(!valid_animation(&target));target.direction=2;target.brightness=101;assert!(!valid_animation(&target));assert!(serde_json::from_str::<Command>(r#"{"type":"restore_lighting","raw_frame":[0,1]}"#).is_err());assert!(serde_json::from_str::<Command>(r#"{"type":"restore_lighting"}"#).is_ok());}
}
