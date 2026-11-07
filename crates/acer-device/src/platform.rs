use super::*;
use serde::Deserialize;
use std::collections::BTreeMap;
use wmi::{Variant,WMIConnection};

pub(super) struct Backend { gaming: WMIConnection, system: WMIConnection, policy: WritePolicy, fan_restore: Option<FanMode>, profile_restore: Option<Profile>, lighting_restore:Option<BacklightFrame>, color_restore:Option<BacklightFrame>, battery_restore:Option<BatteryCareState>, dynamic_restore:Option<BacklightFrame>, previous_lighting:Option<BacklightFrame> }
fn error(e: impl std::fmt::Display)->DeviceError {
    let message=e.to_string(); let lower=message.to_lowercase();
    let kind=if lower.contains("80041003")||lower.contains("80070005")||lower.contains("access is denied")||lower.contains("拒绝访问") {ErrorKind::PermissionDenied} else {ErrorKind::Transport};
    DeviceError{kind,message}
}
fn raw(v:Variant)->RawValue { match v {
    Variant::Bool(b)=>RawValue::Bool(b), Variant::UI1(n)=>RawValue::unsigned(n as u64),Variant::UI2(n)=>RawValue::unsigned(n as u64),Variant::UI4(n)=>RawValue::unsigned(n as u64),Variant::UI8(n)=>RawValue::unsigned(n),
    Variant::I1(n)=>RawValue::Signed(n.to_string()),Variant::I2(n)=>RawValue::Signed(n.to_string()),Variant::I4(n)=>RawValue::Signed(n.to_string()),Variant::I8(n)=>RawValue::Signed(n.to_string()),
    Variant::String(s)=>RawValue::Text(s),Variant::Array(v)=>{if v.iter().all(|v|matches!(v,Variant::UI1(_))) {RawValue::Bytes(v.into_iter().map(|v|if let Variant::UI1(b)=v {b}else{unreachable!()}).collect())}else{RawValue::Array(v.into_iter().map(raw).collect())}},
    Variant::Empty|Variant::Null=>RawValue::Null,other=>RawValue::Text(format!("{other:?}")),
} }
fn protocol(message:&str)->DeviceError{DeviceError{kind:ErrorKind::Protocol,message:message.into()}}
fn supervised_custom_writes(calls:Vec<Call>,mut execute:impl FnMut(Call)->RawRead)->(Vec<RawRead>,bool){let mut writes=vec![];let mut failed=false;for call in calls{let read=execute(call.clone());let ack=supervised_custom_ack(&call,&read);failed|=ack==Some(false);writes.push(read);if ack!=Some(true){break;}}(writes,failed)}
fn lighting_writes(calls:Vec<Call>,keep_going:bool,mut execute:impl FnMut(Call)->RawRead)->(Vec<RawRead>,bool,bool){let mut writes=vec![];let mut failed=false;let mut all_acked=true;for call in calls{let read=execute(call.clone());let ack=lighting_call_ack(&call,&read);failed|=ack==Some(false);all_acked&=ack==Some(true);writes.push(read);if ack!=Some(true)&&!keep_going{break;}}(writes,failed,all_acked)}
impl Backend {
    pub(crate) fn restore_thermal_auto(&mut self)->Result<ApplyReport,DeviceError> {
        // Only the private armed session calls this. Do not gate recovery on a
        // successful sensor read after a partial hardware/transport failure.
        let write=self.call(encode(&Command::SetFanMode{mode:FanMode::Auto})?.remove(0));
        let after=self.snapshot().ok();
        let writes=vec![write];
        Ok(ApplyReport {status:verified_fan_result(FanMode::Auto,&writes,after.as_ref()),message:"Thermal controller Auto recovery attempt".into(),before:None,after,writes})
    }
    pub fn connect(policy:WritePolicy)->Result<Self,DeviceError>{Ok(Self{gaming:WMIConnection::with_namespace_path("ROOT\\WMI").map_err(error)?,system:WMIConnection::with_namespace_path("ROOT\\CIMV2").map_err(error)?,policy,fan_restore:None,profile_restore:None,lighting_restore:None,color_restore:None,battery_restore:None,dynamic_restore:None,previous_lighting:None})}
    pub(super) fn call(&self,call:Call)->RawRead {
        let result=(||->Result<BTreeMap<String,RawValue>,DeviceError>{
            // Class and method are generated from reviewed protocol constants only.
            #[derive(Deserialize)] struct Instance { #[serde(rename="__PATH")] path: String }
            let rows:Vec<Instance>=self.gaming.raw_query(format!("SELECT __PATH FROM {}",call.class)).map_err(error)?;
            if rows.len()!=1{return Err(protocol("Expected exactly one Acer WMI instance"));}
            let path=&rows[0].path;
            let input=self.gaming.get_object(&call.class).map_err(error)?.get_method(&call.method).map_err(error)?.ok_or_else(||protocol("Missing method input signature"))?.spawn_instance().map_err(error)?;
            for (name,value) in &call.input {
                let variant=match value {
                    RawValue::Bytes(v)=>Variant::Array(v.iter().copied().map(Variant::UI1).collect()),
                    RawValue::Unsigned(s)=>{let n=s.parse::<u64>().map_err(error)?;
                        if call.class=="BatteryControl" {Variant::UI1(u8::try_from(n).map_err(error)?)}
                        else if call.method.starts_with("Get") {Variant::UI4(u32::try_from(n).map_err(error)?)} else {Variant::UI8(n)}},
                    _=>return Err(protocol("Unexpected WMI input type")),
                }; input.put_property(name,variant).map_err(error)?;
            }
            let output=self.gaming.exec_method(path,&call.method,Some(&input)).map_err(error)?.ok_or_else(||protocol("WMI returned no output"))?;
            let mut fields=BTreeMap::new();for name in output.list_properties().map_err(error)? {fields.insert(name.clone(),raw(output.get_property(&name).map_err(error)?));}Ok(fields)
        })();
        match result {Ok(output)=>RawRead{class:call.class,method:call.method,input:call.input,output,error:None},Err(e)=>RawRead{class:call.class,method:call.method,input:call.input,output:BTreeMap::new(),error:Some(e)}}
    }
    pub(super) fn identity(&self)->(String,String) {
        #[derive(Deserialize)] #[allow(non_snake_case)] struct Machine{Model:String}
        #[derive(Deserialize)] #[allow(non_snake_case)] struct Bios{SMBIOSBIOSVersion:String}
        let model=self.system.raw_query::<Machine>("SELECT Model FROM Win32_ComputerSystem").ok().and_then(|v|v.into_iter().next()).map(|m|m.Model).unwrap_or_default();
        let bios=self.system.raw_query::<Bios>("SELECT SMBIOSBIOSVersion FROM Win32_BIOS").ok().and_then(|v|v.into_iter().next()).map(|m|m.SMBIOSBIOSVersion).unwrap_or_default();(model,bios)
    }
    fn query_read(connection:&WMIConnection,class:&str,query:&str)->RawRead {
        let result=connection.raw_query::<BTreeMap<String,Variant>>(query).map_err(error).and_then(|rows|{
            rows.into_iter().next().map(|row|row.into_iter().map(|(name,value)|(name,raw(value))).collect()).ok_or_else(||protocol("WMI query returned no instances"))
        });
        match result{Ok(output)=>RawRead{class:class.into(),method:query.into(),input:BTreeMap::new(),output,error:None},Err(e)=>RawRead{class:class.into(),method:query.into(),input:BTreeMap::new(),output:BTreeMap::new(),error:Some(e)}}
    }
    pub(super) fn calibration_power(&self)->(Option<bool>,Option<u8>,Vec<RawRead>){
        let power=Self::query_read(&self.gaming,"BatteryStatus","SELECT PowerOnline FROM BatteryStatus");
        let battery=Self::query_read(&self.system,"Win32_Battery","SELECT EstimatedChargeRemaining FROM Win32_Battery");
        let ac=if power.error.is_none(){match power.output.get("PowerOnline"){Some(RawValue::Bool(value))=>Some(*value),_=>None}}else{None};
        let percent=if battery.error.is_none(){battery.output.get("EstimatedChargeRemaining").and_then(RawValue::as_u64).filter(|n|*n<=100).map(|n|n as u8)}else{None};
        (ac,percent,vec![power,battery])
    }
    pub fn snapshot(&mut self)->Result<Snapshot,DeviceError>{
        let mut reads=vec![self.call(gaming("GetGamingSysInfo",513)),self.call(gaming("GetGamingSysInfo",1537)),self.call(gaming("GetGamingMiscSetting",11)),self.call(gaming("GetGamingFanBehavior",1)),self.call(gaming("GetGamingFanBehavior",8)),self.call(gaming("GetGamingKBBacklight",1))];
        for zone in [1,2,4,8] {reads.push(self.call(gaming("GetGamingRgbKb",zone)));}
        for method in ["GetGamingLEDBehavior","GetGamingLEDColor"] {reads.push(self.call(gaming(method,1)));}
        reads.push(self.call(gaming("GetGamingMiscSetting",10)));
        for selector in [1,4] {reads.push(self.call(gaming("GetGamingFanSpeed",selector)));}
        reads.push(self.call(battery_read()));
        let profile=reads[2].scalar().and_then(Profile::decode);let care=reads.last().and_then(battery_care);
        let lighting=static_lighting(&reads[5],&reads[6..10]);
        let lighting_effect=backlight_effect(&reads[5]);
        let mode=reads[3].scalar().zip(reads[4].scalar()).and_then(|(cpu,gpu)|fan_mode(cpu,gpu));
        let cached_percent=|selector:u64|reads.iter().find(|r|r.method=="GetGamingFanSpeed"&&r.input.get("gmInput").and_then(RawValue::as_u64)==Some(selector)).and_then(RawRead::scalar).and_then(fan_percent);
        let cpu_fan_percent=cached_percent(1);let gpu_fan_percent=cached_percent(4);
        reads.push(self.call(gaming("GetGamingFanBehavior",9)));
        let sensor_mask=self.call(gaming("GetGamingSysInfo",0));let mask=sensor_mask.scalar();reads.push(sensor_mask);
        let mut temps=[None,None];for (index,id) in [1u8,10].into_iter().enumerate(){if mask.is_some_and(|m|sensor_supported(m,id)){let read=self.call(gaming("GetGamingSysInfo",1|((id as u64)<<8)));temps[index]=read.scalar().and_then(temperature);reads.push(read);}}
        // Fan RPM scaling is from fixed upstream, not yet compared against official UI.
        let cpu_fan_rpm=reads[0].scalar().and_then(fan_rpm);let gpu_fan_rpm=reads[1].scalar().and_then(fan_rpm);
        let power_read=Self::query_read(&self.gaming,"BatteryStatus","SELECT PowerOnline FROM BatteryStatus");
        let power=match power_read.output.get("PowerOnline"){Some(RawValue::Bool(value))=>Some(*value),_=>None};
        let battery_read=Self::query_read(&self.system,"Win32_Battery","SELECT EstimatedChargeRemaining FROM Win32_Battery");
        let battery=battery_read.output.get("EstimatedChargeRemaining").and_then(RawValue::as_u64).filter(|n|*n<=100).map(|n|n as u8);
        reads.push(power_read);reads.push(battery_read);
        let lighting_config=lighting_config_from_reads(&reads);
        Ok(Snapshot{captured_at_ms:std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as u64,cpu_temperature_c:temps[0],gpu_temperature_c:temps[1],cpu_fan_rpm,gpu_fan_rpm,cpu_fan_percent,gpu_fan_percent,profile,fan_mode:mode,lighting,lighting_effect,lighting_config,battery_care:care,ac_online:power,battery_percent:battery,reads})
    }
    pub(crate) fn thermal_snapshot(&mut self)->Result<Snapshot,DeviceError>{
        let mut reads=vec![];
        let mut get=|method:&str,selector|{let read=self.call(gaming(method,selector));let value=read.scalar();reads.push(read);value};
        let cpu_fan_rpm=get("GetGamingSysInfo",513).and_then(fan_rpm);
        let gpu_fan_rpm=get("GetGamingSysInfo",1537).and_then(fan_rpm);
        let cpu_mode=get("GetGamingFanBehavior",1);let gpu_mode=get("GetGamingFanBehavior",8);
        get("GetGamingFanBehavior",9);
        let mode=cpu_mode.zip(gpu_mode).and_then(|(cpu,gpu)|fan_mode(cpu,gpu));
        let cpu_fan_percent=get("GetGamingFanSpeed",1).and_then(fan_percent);
        let gpu_fan_percent=get("GetGamingFanSpeed",4).and_then(fan_percent);
        let mask=get("GetGamingSysInfo",0);
        let cpu_temperature_c=if mask.is_some_and(|m|sensor_supported(m,1)){get("GetGamingSysInfo",0x101).and_then(temperature)}else{None};
        let gpu_temperature_c=if mask.is_some_and(|m|sensor_supported(m,10)){get("GetGamingSysInfo",0xA01).and_then(temperature)}else{None};
        let power_read=Self::query_read(&self.gaming,"BatteryStatus","SELECT PowerOnline FROM BatteryStatus");
        let ac_online=if power_read.error.is_none(){match power_read.output.get("PowerOnline"){Some(RawValue::Bool(v))=>Some(*v),_=>None}}else{None};
        reads.push(power_read);
        Ok(Snapshot {captured_at_ms:std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as u64,cpu_temperature_c,gpu_temperature_c,cpu_fan_rpm,gpu_fan_rpm,cpu_fan_percent,gpu_fan_percent,fan_mode:mode,ac_online,profile:None,lighting:None,lighting_effect:None,lighting_config:None,battery_care:None,battery_percent:None,reads})
    }
    pub fn capabilities(&mut self)->Result<Capabilities,DeviceError>{
        let(model,bios)=self.identity();let matched_model=model=="Predator PHN16-71"||model=="PHN16-71";
        let snapshot=self.snapshot()?;
        let fan_verified=verified_fan_target(&model,&bios);
        let fan_writable=fan_verified&&self.policy==WritePolicy::Verified&&fan_control_ready(&snapshot);
        let custom_writable=fan_verified&&self.policy==WritePolicy::Verified&&verified_custom_ready(&snapshot,70,70);
        let profile_writable=fan_verified&&self.policy==WritePolicy::Verified&&[Profile::Quiet,Profile::Balanced,Profile::Performance,Profile::Turbo].into_iter().any(|p|verified_profile_ready(&snapshot,p));
        let lighting_writable=fan_verified&&self.policy==WritePolicy::Verified&&lighting_control_ready(&snapshot);
        let lighting_restore_available=self.previous_lighting.is_some();
        let battery_writable=fan_verified&&self.policy==WritePolicy::Verified&&battery_validation_state(&snapshot).is_some();
        let items=[("temperature",snapshot.cpu_temperature_c.is_some()||snapshot.gpu_temperature_c.is_some()),("fan_rpm",snapshot.cpu_fan_rpm.is_some()&&snapshot.gpu_fan_rpm.is_some()),("profile",snapshot.profile.is_some()),("fan_mode",snapshot.fan_mode.is_some()),("fan_custom",snapshot.cpu_fan_percent.is_some()&&snapshot.gpu_fan_percent.is_some()),("lighting",snapshot.lighting_config.is_some()),("lighting_dynamic",snapshot.lighting_config.is_some()),("lighting_restore",lighting_restore_available),("battery_care",snapshot.battery_care.is_some())].into_iter().map(|(id,readable)|{
            let pending=matches!(id,"lighting");
            let permission=snapshot.reads.iter().filter_map(|r|r.error.as_ref()).find(|e|e.kind==ErrorKind::PermissionDenied);
            Capability{id:id.into(),readable,writable:(id=="fan_mode"&&fan_writable)||(id=="profile"&&profile_writable)||(id=="fan_custom"&&custom_writable)||(matches!(id,"lighting"|"lighting_dynamic")&&lighting_writable)||(id=="lighting_restore"&&lighting_restore_available&&self.policy==WritePolicy::Verified)||(id=="battery_care"&&battery_writable),status:if id=="lighting_restore"&&lighting_restore_available{CapabilityStatus::Verified}else if matches!(id,"fan_mode"|"fan_custom"|"profile"|"lighting"|"lighting_dynamic"|"battery_care")&&fan_verified&&readable{CapabilityStatus::Verified}else if !readable&&permission.is_some(){CapabilityStatus::PermissionBlocked}else if pending{CapabilityStatus::ProtocolPending}else{CapabilityStatus::ImplementedUnverified},reason:if id=="lighting_restore"{"Restore uses only the server-held previous lighting receipt; no caller raw frame; session-only availability".into()}else if matches!(id,"lighting"|"lighting_dynamic")&&fan_verified&&readable{"Effects0..7 have individual trial/restore evidence; static/dynamic transitions require a known restorable frame, per-setter ACK and requested configuration readback; static zones may be unknown during animation".into()}else if id=="battery_care"&&fan_verified&&readable{"Health-mode bool inversion/restore verified; supported function1 and idle other functions required, UInt16 ACK and state readback checked; physical80% charge threshold was not measured".into()}else if id=="profile"&&fan_verified&&readable{"Quiet/Balanced/Performance/Turbo verified on AC; writes require native Verified policy, battery>=40%, supported mask and independent profile checks; Eco remains untested".into()}else if id=="fan_custom"&&fan_verified&&readable{"Official local IPC range is 0..100. Only the70% physical point was tested; every request requires behavior and both target-percentage readback. Cached targets are not physical duty measurements.".into()}else if id=="fan_mode"&&fan_verified&&readable{if fan_writable{"PHN16-71 / V1.18 Auto/Max validated; current Custom may switch back using exact firmware ACK and three matching mode reads".into()}else{"Auto/Max has target-specific validation evidence; current native policy or current read state does not permit writing".into()}}else if readable {"Read succeeded; mapping is from fixed reference and awaits official UI comparison. Hardware writes have not been validated.".into()} else if pending {"Protocol pending: continue source analysis; this is not evidence of unsupported hardware".into()} else {snapshot.reads.iter().filter_map(|r|r.error.as_ref()).next().map(|e|e.to_string()).unwrap_or_else(||"Mapping or firmware status has not been verified on this machine".into())}}
        }).collect();
        Ok(Capabilities{model,bios,matched_model,items})
    }
    pub fn apply(&mut self,command:Command)->Result<ApplyReport,DeviceError>{
        let reject=|message:String|ApplyReport{status:ApplyStatus::Rejected,message,before:None,after:None,writes:vec![]};
        if matches!(self.policy,WritePolicy::ReadOnly){return Ok(reject("Read-only policy: native caller must explicitly opt in for supervised hardware validation".into()));}
        if matches!(command,Command::RestoreLighting{}){return self.restore_previous_lighting();}
        let(model,bios)=self.identity();if !(model=="Predator PHN16-71"||model=="PHN16-71")||bios!="V1.18"{return Ok(reject(format!("Unsupported model/BIOS: {model} / {bios}")));}
        if !policy_allows(self.policy,&model,&bios,&command){return Ok(reject("Verified target whitelist or argument range not satisfied; Eco remains outside the normal whitelist".into()));}
        let calls=match encode(&command){Ok(c)=>c,Err(e)=>return Ok(reject(e.to_string()))};
        let before=self.snapshot()?;
        if let Command::SetProfile{profile}=command {if !profile_power_ready(&before)||!profile_supported(&before,profile){return Ok(reject("Profile requires confirmed AC, battery >=40% and a supported target mode".into()));}if matches!(profile,Profile::Balanced|Profile::Quiet|Profile::Turbo)&&self.policy==WritePolicy::AllowUnverified {if !profile_trial_ready(&before,profile){return Ok(reject("Profile validation requires a supported non-Eco original mode and restorable power conditions".into()));}if self.profile_restore.is_none(){self.profile_restore=before.profile;}}}
        if self.policy==WritePolicy::Verified{let ready=match command{Command::SetFanMode{..}=>fan_control_ready(&before),Command::SetFanSpeeds{cpu_percent,gpu_percent}=>verified_custom_ready(&before,cpu_percent,gpu_percent),Command::SetProfile{profile}=>verified_profile_ready(&before,profile),Command::SetLighting{ref lighting}=>verified_static_ready(&before,lighting),Command::SetLightingAnimation{ref lighting}=>valid_animation(lighting)&&lighting_control_ready(&before),Command::SetBatteryCare{..}=>battery_validation_state(&before).is_some(),Command::RestoreLighting{}=>unreachable!("handled receipt restore before reads")};if !ready{return Ok(reject("Current state does not satisfy the independent Verified guard for this command".into()));}}
        if self.policy==WritePolicy::AllowUnverified&&matches!(command,Command::SetFanMode{mode:FanMode::Max})&&!verified_fan_ready(&before){return Ok(reject("Supervised Max trial requires original Auto/Max; Custom or unknown must not be overwritten".into()));}
        let can_read=match &command{Command::SetProfile{..}=>before.profile.is_some(),Command::SetFanMode{..}|Command::SetFanSpeeds{..}=>before.fan_mode.is_some(),Command::SetBatteryCare{..}=>battery_validation_state(&before).is_some(),Command::SetLighting{..}|Command::SetLightingAnimation{..}=>self.policy==WritePolicy::Verified&&lighting_control_ready(&before),Command::RestoreLighting{}=>unreachable!("handled receipt restore")};
        if !can_read{return Ok(reject("Cannot read current setting and confirm restore value".into()));}
        if matches!(command,Command::SetProfile{profile:Profile::Performance|Profile::Turbo})&&before.ac_online!=Some(true){return Ok(reject("Performance/Turbo requires confirmed AC power".into()));}
        if matches!(command,Command::SetFanMode{mode:FanMode::Max})&&matches!(before.fan_mode,Some(FanMode::Auto|FanMode::Max))&&self.fan_restore.is_none(){self.fan_restore=before.fan_mode;}
        let calls=if self.policy==WritePolicy::Verified{if let Command::SetLighting{ref lighting}=command{static_lighting_plan(&observed_lighting_frame(&before).expect("known lighting frame"),lighting).expect("validated static target")}else{calls}}else{calls};
        if self.policy==WritePolicy::Verified&&matches!(command,Command::SetLighting{..}|Command::SetLightingAnimation{..}){self.previous_lighting=observed_lighting_frame(&before);}
        let(writes,failed)=if self.policy==WritePolicy::Verified&&matches!(command,Command::SetFanSpeeds{..}){supervised_custom_writes(calls,|call|self.call(call))}else if self.policy==WritePolicy::Verified&&matches!(command,Command::SetLighting{..}){let(w,f,_)=lighting_writes(calls,false,|call|self.call(call));(w,f)}else{let mut writes=vec![];for call in calls {let r=self.call(call);let ok=r.firmware_write_ok()==Some(true);writes.push(r);if !ok{break;}}let failed=writes.iter().any(|r|r.firmware_write_ok()==Some(false));(writes,failed)};
        let after=self.snapshot().ok();
        let status=if self.policy==WritePolicy::Verified{match command{Command::SetFanMode{mode}=>verified_fan_result(mode,&writes,after.as_ref()),Command::SetFanSpeeds{cpu_percent,gpu_percent}=>verified_custom_result(cpu_percent,gpu_percent,&writes,after.as_ref()),Command::SetProfile{profile}=>verified_profile_result(profile,&writes,after.as_ref()),Command::SetLighting{ref lighting}=>verified_static_result(lighting,&observed_lighting_frame(&before).expect("known frame"),&writes,after.as_ref()),Command::SetLightingAnimation{ref lighting}=>verified_animation_result(lighting,&writes,after.as_ref()),Command::SetBatteryCare{enabled}=>verified_battery_result(enabled,&battery_validation_state(&before).expect("battery guard"),&writes,after.as_ref()),Command::RestoreLighting{}=>unreachable!("handled receipt restore")}}else if failed{ApplyStatus::Failed}else{ApplyStatus::Unconfirmed};
        let report=ApplyReport{status,message:match status{ApplyStatus::Applied=>"Verified fixed setter ACK and independent target readback matched",ApplyStatus::Failed=>"WMI or firmware rejected the write; inspect raw return/error",_=>"Write or readback is unconfirmed; inspect raw firmware ACK and target reads"}.into(),before:Some(before),after,writes};
        if self.policy==WritePolicy::Verified && matches!(command,Command::SetFanSpeeds{..}) {
            // Model/policy were checked before the transaction. Failed sensor reads
            // must not block this single fixed recovery write; do not recurse apply().
            return Ok(crate::fan_recovery::recover_custom_failure(report,|mode| {
                let call=encode(&Command::SetFanMode{mode}).expect("Auto/Max recovery encoding").remove(0);
                let write=self.call(call);
                (write,self.snapshot().ok())
            }));
        }
        Ok(report)
    }
    pub fn restore_validation_fan(&mut self)->Result<ApplyReport,DeviceError>{
        let reject=|message:&str|ApplyReport{status:ApplyStatus::Rejected,message:message.into(),before:None,after:None,writes:vec![]};
        if matches!(self.policy,WritePolicy::ReadOnly){return Ok(reject("Read-only policy forbids validation restore"));}
        let Some(mode)=self.fan_restore else{return Ok(reject("No verified original Auto/Max state was saved before a Max attempt"));};
        // Target/model/BIOS and original mode were checked before arming the receipt.
        // A failed current read MUST NOT prevent the attempt to restore that state.
        let before=self.snapshot().ok();
        let call=encode(&Command::SetFanMode{mode})?.remove(0);
        let write=self.call(call);let failed=write.firmware_write_ok()==Some(false);
        let after=self.snapshot().ok();
        let writes=vec![write];
        let status=if self.policy==WritePolicy::Verified{verified_fan_result(mode,&writes,after.as_ref())}else if failed{ApplyStatus::Failed}else{ApplyStatus::Unconfirmed};
        let confirmed=if self.policy==WritePolicy::Verified{status==ApplyStatus::Applied}else{!failed&&after.as_ref().is_some_and(|s|s.fan_mode==Some(mode))};
        if confirmed{self.fan_restore=None;}
        Ok(ApplyReport{status,message:"Attempted restoration to the saved pre-validation fan mode; inspect raw write and readback".into(),before,after,writes})
    }
    pub fn restore_validation_profile(&mut self)->Result<ApplyReport,DeviceError>{
        let reject=|message:&str,before:Option<Snapshot>|ApplyReport{status:ApplyStatus::Rejected,message:message.into(),before,after:None,writes:vec![]};
        if self.policy!=WritePolicy::AllowUnverified{return Ok(reject("Profile restore requires native supervised validation policy",None));}
        let Some(profile)=self.profile_restore else{return Ok(reject("No original profile receipt was saved",None));};
        let before=self.snapshot()?;
        if !profile_power_ready(&before)||!profile_supported(&before,profile){return Ok(reject("Original profile remains pending restore: AC/battery>=40%/supported target cannot be confirmed",Some(before)));}
        let write=self.call(encode(&Command::SetProfile{profile})?.remove(0));let failed=write.firmware_write_ok()==Some(false);let after=self.snapshot().ok();
        if !failed&&after.as_ref().is_some_and(|s|s.profile==Some(profile)){self.profile_restore=None;}
        Ok(ApplyReport{status:if failed{ApplyStatus::Failed}else{ApplyStatus::Unconfirmed},message:"Attempted saved original profile restore; inspect firmware return and mode readback".into(),before:Some(before),after,writes:vec![write]})
    }
    pub fn begin_fan_custom_70_validation(&mut self)->Result<ApplyReport,DeviceError>{
        let reject=|message:&str|ApplyReport{status:ApplyStatus::Rejected,message:message.into(),before:None,after:None,writes:vec![]};
        if self.policy!=WritePolicy::AllowUnverified{return Ok(reject("Custom validation requires native supervised policy"));}
        let(model,bios)=self.identity();if !verified_fan_target(&model,&bios){return Ok(reject("Custom validation requires PHN16-71 / V1.18"));}
        if self.fan_restore.is_some(){return Ok(reject("A previous fan restore is pending; do not begin another trial"));}
        let before=self.snapshot()?;if !verified_fan_ready(&before){return Ok(reject("Custom validation requires original Auto/Max with CPU/GPU/combined state and both RPM reads"));}
        self.fan_restore=before.fan_mode;
        let calls=encode(&Command::SetFanSpeeds{cpu_percent:70,gpu_percent:70})?;
        let(writes,failed)=supervised_custom_writes(calls,|call|self.call(call));
        let after=self.snapshot().ok();
        Ok(ApplyReport{status:if failed{ApplyStatus::Failed}else{ApplyStatus::Unconfirmed},message:"Supervised Custom 70/70 trial only; inspect all three raw writes and Custom/percentage readbacks, then restore saved Auto/Max".into(),before:Some(before),after,writes})
    }
    pub fn begin_lighting_brightness_validation(&mut self)->Result<ApplyReport,DeviceError>{
        let reject=|message:&str|ApplyReport{status:ApplyStatus::Rejected,message:message.into(),before:None,after:None,writes:vec![]};
        if self.policy!=WritePolicy::AllowUnverified{return Ok(reject("Lighting validation requires native supervised policy"));}
        let(model,bios)=self.identity();if !verified_fan_target(&model,&bios){return Ok(reject("Lighting validation requires PHN16-71/V1.18"));}
        if self.lighting_restore.is_some(){return Ok(reject("An earlier lighting restore is pending"));}
        let before=self.snapshot()?;let Some(saved)=observed_backlight_frame(&before)else{return Ok(reject("Requires static0, valid getter15+gmReturn0 and four readable zones"));};
        let frame=brightness_frame(&saved,20).expect("validated fixed brightness frame");self.lighting_restore=Some(saved);
        let write=self.call(backlight_frame_call(frame.clone()));let failed=backlight_frame_ack(&write,&frame)==Some(false);let after=self.snapshot().ok();
        Ok(ApplyReport{status:if failed{ApplyStatus::Failed}else{ApplyStatus::Unconfirmed},message:"Supervised brightness20 only; setter16 reconstructed from getter15 using local DLL routing/padding; original frame receipt saved before write".into(),before:Some(before),after,writes:vec![write]})
    }
    pub fn restore_validation_lighting(&mut self)->Result<ApplyReport,DeviceError>{
        let reject=|message:&str|ApplyReport{status:ApplyStatus::Rejected,message:message.into(),before:None,after:None,writes:vec![]};
        if self.policy!=WritePolicy::AllowUnverified{return Ok(reject("Lighting restore requires native supervised policy"));}
        let Some(saved)=self.lighting_restore.clone()else{return Ok(reject("No saved original lighting frame"));};
        let before=self.snapshot().ok();let frame=saved.setter_frame.clone();let write=self.call(backlight_frame_call(frame.clone()));let ack=backlight_frame_ack(&write,&frame);let after=self.snapshot().ok();
        if ack==Some(true)&&after.as_ref().is_some_and(|s|backlight_frame_matches(s,&saved,saved.observed_state[2])){self.lighting_restore=None;}
        Ok(ApplyReport{status:if ack==Some(false){ApplyStatus::Failed}else{ApplyStatus::Unconfirmed},message:"Attempted saved original lighting frame restore even if current effect/read is unknown; compare normalized state and four original zone words".into(),before,after,writes:vec![write]})
    }
    pub fn begin_static_colors_validation(&mut self)->Result<ApplyReport,DeviceError>{
        let reject=|message:&str|ApplyReport{status:ApplyStatus::Rejected,message:message.into(),before:None,after:None,writes:vec![]};
        if self.policy!=WritePolicy::AllowUnverified{return Ok(reject("Static color validation requires native supervised policy"));}
        let(model,bios)=self.identity();if !verified_fan_target(&model,&bios){return Ok(reject("Requires PHN16-71/V1.18"));}
        if self.color_restore.is_some()||self.lighting_restore.is_some(){return Ok(reject("An earlier lighting restore is pending"));}
        let before=self.snapshot()?;let Some(saved)=observed_backlight_frame(&before)else{return Ok(reject("Requires static frame and four readable zones"));};
        if static_color_restore_plan(&saved).is_none(){return Ok(reject("Original four zone words cannot be safely restored"));}
        let calls=static_color_trial_plan(&saved).expect("validated fixed color plan");self.color_restore=Some(saved);
        let(writes,failed,_)=lighting_writes(calls,false,|call|self.call(call));
        let after=self.snapshot().ok();Ok(ApplyReport{status:if failed{ApplyStatus::Failed}else{ApplyStatus::Unconfirmed},message:"Supervised static red/green/blue/white zones at brightness20; saved original four zones and normalized frame before first write".into(),before:Some(before),after,writes})
    }
    pub fn restore_validation_static_colors(&mut self)->Result<ApplyReport,DeviceError>{
        let reject=|message:&str|ApplyReport{status:ApplyStatus::Rejected,message:message.into(),before:None,after:None,writes:vec![]};
        if self.policy!=WritePolicy::AllowUnverified{return Ok(reject("Static color restore requires native supervised policy"));}
        let Some(saved)=self.color_restore.clone()else{return Ok(reject("No saved static color receipt"));};
        let before=self.snapshot().ok();let calls=static_color_restore_plan(&saved).expect("receipt validated before trial");
        // Restore every saved zone and the frame even if an earlier restore step fails.
        let(writes,failed,all_acked)=lighting_writes(calls,true,|call|self.call(call));
        let after=self.snapshot().ok();if all_acked&&after.as_ref().is_some_and(|s|backlight_frame_matches(s,&saved,saved.observed_state[2])){self.color_restore=None;}
        Ok(ApplyReport{status:if failed{ApplyStatus::Failed}else{ApplyStatus::Unconfirmed},message:"Attempted all four saved zone restores and the saved frame, regardless of intermediate restoration failures".into(),before,after,writes})
    }
    pub fn begin_battery_care_validation(&mut self)->Result<ApplyReport,DeviceError>{
        let reject=|message:&str|ApplyReport{status:ApplyStatus::Rejected,message:message.into(),before:None,after:None,writes:vec![]};
        if self.policy!=WritePolicy::AllowUnverified{return Ok(reject("Battery trial requires native supervised policy"));}
        let(model,bios)=self.identity();if !verified_fan_target(&model,&bios){return Ok(reject("Battery trial requires PHN16-71/V1.18"));}
        if self.battery_restore.is_some(){return Ok(reject("Earlier battery-care restoration is pending"));}
        let before=self.snapshot()?;let Some(saved)=battery_validation_state(&before)else{return Ok(reject("Requires supported/reliable health-mode bool and idle other battery functions; no calibration operation is allowed"));};
        let target=!saved.enabled;self.battery_restore=Some(saved);let write=self.call(encode(&Command::SetBatteryCare{enabled:target})?.remove(0));let ack=battery_care_ack(&write,target);let after=self.snapshot().ok();
        Ok(ApplyReport{status:if ack==Some(false){ApplyStatus::Failed}else{ApplyStatus::Unconfirmed},message:"Supervised inversion of health-mode function mask1 only; original state saved before write; physical charge threshold not measured".into(),before:Some(before),after,writes:vec![write]})
    }
    pub fn restore_validation_battery_care(&mut self)->Result<ApplyReport,DeviceError>{
        let reject=|message:&str|ApplyReport{status:ApplyStatus::Rejected,message:message.into(),before:None,after:None,writes:vec![]};
        if self.policy!=WritePolicy::AllowUnverified{return Ok(reject("Battery restore requires native supervised policy"));}
        let Some(saved)=self.battery_restore.clone()else{return Ok(reject("No original battery-care receipt saved"));};
        let before=self.snapshot().ok();let write=self.call(encode(&Command::SetBatteryCare{enabled:saved.enabled})?.remove(0));let ack=battery_care_ack(&write,saved.enabled);let after=self.snapshot().ok();
        if ack==Some(true)&&after.as_ref().is_some_and(|s|battery_care_readback(s,&saved,saved.enabled)){self.battery_restore=None;}
        Ok(ApplyReport{status:if ack==Some(false){ApplyStatus::Failed}else{ApplyStatus::Unconfirmed},message:"Attempted original health-mode bool restore using function mask1, independent of current read success; other function states must remain unchanged".into(),before,after,writes:vec![write]})
    }
    pub fn save_validation_lighting_checkpoint(&mut self)->Result<ApplyReport,DeviceError>{
        let reject=|message:&str|ApplyReport{status:ApplyStatus::Rejected,message:message.into(),before:None,after:None,writes:vec![]};
        if self.policy!=WritePolicy::AllowUnverified{return Ok(reject("Lighting checkpoint requires native supervised policy"));}
        let(model,bios)=self.identity();if !verified_fan_target(&model,&bios){return Ok(reject("Requires PHN16-71/V1.18"));}
        if self.dynamic_restore.is_some()||self.color_restore.is_some()||self.lighting_restore.is_some(){return Ok(reject("Earlier lighting restore is pending"));}
        let before=self.snapshot()?;let Some(saved)=observed_backlight_frame(&before)else{return Ok(reject("Original static0 and all four zones must be readable"));};
        if dynamic_restore_plan(&saved).is_none(){return Ok(reject("Original lighting state cannot be safely restored"));}
        self.dynamic_restore=Some(saved);
        Ok(ApplyReport{status:ApplyStatus::Applied,message:"Saved read-only original static checkpoint in this worker; no hardware write".into(),before:Some(before),after:None,writes:vec![]})
    }
    pub fn begin_dynamic_lighting_validation(&mut self,effect:LightingEffect)->Result<ApplyReport,DeviceError>{
        let reject=|message:&str|ApplyReport{status:ApplyStatus::Rejected,message:message.into(),before:None,after:None,writes:vec![]};
        if self.policy!=WritePolicy::AllowUnverified{return Ok(reject("Dynamic trial requires native supervised policy"));}
        let(model,bios)=self.identity();if !verified_fan_target(&model,&bios){return Ok(reject("Requires PHN16-71/V1.18"));}
        let Some(frame)=dynamic_trial_frame(effect)else{return Ok(reject("Static0 already validated; dynamic trial target must be1..7"));};
        if self.dynamic_restore.is_some()||self.color_restore.is_some()||self.lighting_restore.is_some(){return Ok(reject("Earlier lighting restore is pending"));}
        let before=self.snapshot()?;let Some(saved)=observed_backlight_frame(&before)else{return Ok(reject("Original static0 and all four zones must be readable"));};if dynamic_restore_plan(&saved).is_none(){return Ok(reject("Original lighting state cannot be safely restored"));}
        self.dynamic_restore=Some(saved);let write=self.call(backlight_frame_call(frame.clone()));let ack=backlight_frame_ack(&write,&frame);let after=self.snapshot().ok();
        Ok(ApplyReport{status:if ack==Some(false){ApplyStatus::Failed}else{ApplyStatus::Unconfirmed},message:"One supervised dynamic effect only: speed3 brightness20 right direction, orange where used; original static frame and zones saved before write".into(),before:Some(before),after,writes:vec![write]})
    }
    fn restore_previous_lighting(&mut self)->Result<ApplyReport,DeviceError>{
        let reject=|message:&str|ApplyReport{status:ApplyStatus::Rejected,message:message.into(),before:None,after:None,writes:vec![]};
        if self.policy!=WritePolicy::Verified{return Ok(reject("Saved lighting recovery requires native Verified policy"));}
        // This receipt can only be created after the exact model/BIOS guard.
        // Recovering it must not depend on a successful fresh lighting read.
        let Some(saved)=self.previous_lighting.clone()else{return Ok(reject("No previous lighting receipt in this helper session"));};let before=self.snapshot().ok();let calls=lighting_restore_plan(&saved).expect("validated before receipt creation");let(writes,_,_)=lighting_writes(calls,true,|call|self.call(call));let after=self.snapshot().ok();let status=verified_lighting_restore_result(&saved,&writes,after.as_ref());if status==ApplyStatus::Applied{self.previous_lighting=None;}Ok(ApplyReport{status,message:if status==ApplyStatus::Applied{"Saved lighting configuration readback confirmed"}else{"Saved lighting recovery unconfirmed; receipt retained for retry"}.into(),before,after,writes})
    }
    pub fn restore_validation_dynamic_lighting(&mut self)->Result<ApplyReport,DeviceError>{
        let reject=|message:&str|ApplyReport{status:ApplyStatus::Rejected,message:message.into(),before:None,after:None,writes:vec![]};if self.policy!=WritePolicy::AllowUnverified{return Ok(reject("Dynamic restore requires native supervised policy"));}
        let Some(saved)=self.dynamic_restore.clone()else{return Ok(reject("No original dynamic-trial lighting receipt"));};let before=self.snapshot().ok();
        // Stop the animation with the saved static frame at brightness0, then
        // restore all four original zones and the original full-brightness frame.
        let calls=dynamic_restore_plan(&saved).expect("receipt validated");let(writes,failed,all_acked)=lighting_writes(calls,true,|call|self.call(call));let after=self.snapshot().ok();if all_acked&&after.as_ref().is_some_and(|s|backlight_frame_matches(s,&saved,saved.observed_state[2])){self.dynamic_restore=None;}
        Ok(ApplyReport{status:if failed{ApplyStatus::Failed}else{ApplyStatus::Unconfirmed},message:"Attempted static stop, all original zones and saved original frame even after intermediate failures".into(),before,after,writes})
    }
}

#[cfg(test)] mod tests {
    use super::*;
    #[test] fn access_denied_is_not_unsupported_or_zero() {for message in ["HRESULT 0x80041003","0x80070005: Access is denied","拒绝访问"]{assert_eq!(error(message).kind,ErrorKind::PermissionDenied);}assert_eq!(error("RPC server unavailable").kind,ErrorKind::Transport);}
    #[test] fn byte_array_and_uint64_retain_exact_data() {assert_eq!(raw(Variant::UI8(u64::MAX)),RawValue::unsigned(u64::MAX));assert_eq!(raw(Variant::Array(vec![Variant::UI1(0),Variant::UI1(255)])),RawValue::Bytes(vec![0,255]));}
    #[test]fn custom_plan_stops_before_gpu_on_cpu_failure_or_missing_ack(){
        for missing in [false,true]{let calls=encode(&Command::SetFanSpeeds{cpu_percent:70,gpu_percent:70}).unwrap();let mut count=0;let(writes,failed)=supervised_custom_writes(calls,|call|{count+=1;let output=if missing{BTreeMap::new()}else{BTreeMap::from([("gmOutput".into(),RawValue::unsigned(if count==2{2}else{0}))])};RawRead{class:call.class,method:call.method,input:call.input,output,error:None}});assert_eq!(count,if missing{1}else{2});assert_eq!(writes.len(),count);assert_eq!(failed,!missing);}
    }
    #[test]fn restoration_attempts_all_lighting_steps_after_failed_or_missing_ack(){for keep_going in [false,true]{for missing in [false,true]{let calls:Vec<_>=VALIDATION_STATIC_ZONES.into_iter().enumerate().map(|(i,c)|gaming("SetGamingRgbKb",zone_color_word(i,c).unwrap())).chain(std::iter::once(backlight_frame_call(vec![0;16]))).collect();let mut count=0;let(writes,failed,all_acked)=lighting_writes(calls,keep_going,|c|{count+=1;let output=if missing&&count==2{BTreeMap::new()}else{BTreeMap::from([("gmOutput".into(),RawValue::unsigned(if !missing&&count==2{1}else{0}))])};RawRead{class:c.class,method:c.method,input:c.input,output,error:None}});assert_eq!(count,if keep_going{5}else{2});assert_eq!(writes.len(),count);assert_eq!(failed,!missing);assert!(!all_acked);}}}
}
