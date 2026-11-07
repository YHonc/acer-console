//! Fixed local RPC. Elevation must be initiated by an explicit UI action.
use acer_protocol::*;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(all(windows,feature="calibration-research"))] pub mod calibration_runtime;
#[cfg(all(windows,feature="calibration-research"))] pub mod calibration_worker;

const MAX_FRAME: usize = 256 * 1024;
const VERSION: u32 = 1;
/// Fixed lifecycle actions. No paths, WMI arguments or write policy overrides.
#[derive(Debug,Serialize,Deserialize)]
#[serde(tag="action",rename_all="snake_case",deny_unknown_fields)]
#[cfg(feature="calibration-research")]
pub enum CalibrationAction {Start{},Stop{},Abort{},Recover{session_id:String}}
pub(crate) fn error(message: impl ToString) -> DeviceError {
    DeviceError { kind: ErrorKind::Transport, message: message.to_string() }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Request { version: u32, id: u64, operation: Operation }
#[derive(Serialize, Deserialize)]
#[serde(tag = "op", content = "command", rename_all = "snake_case", deny_unknown_fields)]
enum Operation { ThermalStatus, StartThermal(acer_device::ThermalConfig), StopThermal, CentralPlugin, SetCentralGuard {action:acer_system::central_plugin::GuardAction}, Ping, Snapshot, Capabilities, Apply(Command), OfficialTask(OfficialAction), VendorControls, SetVendorControl {setting:VendorSetting,enabled:bool}, UsbCharge, SetUsbCharge(UsbState), Audio, SetAudio {preset:audio_controls::AudioPreset}, BootLogo, PrepareBootLogo(boot_logo::BootLogoAction), CommitBootLogo {plan_id:String}, DiscardBootLogo {plan_id:String} }
impl Operation {fn budget_seconds(&self)->u64 {if matches!(self,Self::PrepareBootLogo(_)|Self::CommitBootLogo{..}){120}else{15}}}
/// Fixed task actions only. No task names, paths, commands or policy overrides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all="snake_case")]
pub enum OfficialAction { Query, Backup, Disable, Restore }
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Response { version: u32, id: u64, result: Result<Payload, DeviceError> }
#[derive(Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
enum Payload { Thermal(acer_device::ThermalStatus), CentralPlugin(serde_json::Value), Pong, Snapshot(Snapshot), Capabilities(Capabilities), Apply(ApplyReport), OfficialTask(serde_json::Value), VendorControls(serde_json::Value), UsbCharge(serde_json::Value), Audio(serde_json::Value), BootLogo(serde_json::Value) }

pub struct BrokerClient {
    #[cfg(windows)] session: platform::Session,
    next_id: AtomicU64,
    call_lock: std::sync::Mutex<()>,
}
impl BrokerClient {
    fn thermal_call(&self,operation:Operation)->Result<acer_device::ThermalStatus,DeviceError>{match self.call(operation)?{Payload::Thermal(status)=>Ok(status),_=>Err(error("Unexpected thermal response"))}}
    pub fn thermal_status(&self)->Result<acer_device::ThermalStatus,DeviceError>{self.thermal_call(Operation::ThermalStatus)}
    pub fn start_thermal(&self,config:acer_device::ThermalConfig)->Result<acer_device::ThermalStatus,DeviceError>{self.thermal_call(Operation::StartThermal(config))}
    pub fn stop_thermal(&self)->Result<acer_device::ThermalStatus,DeviceError>{self.thermal_call(Operation::StopThermal)}
    pub fn central_plugin(&self)->Result<serde_json::Value,DeviceError>{match self.call(Operation::CentralPlugin)?{Payload::CentralPlugin(value)=>Ok(value),_=>Err(error("Unexpected plugin response"))}}
    pub fn set_central_guard(&self,action:acer_system::central_plugin::GuardAction)->Result<serde_json::Value,DeviceError>{match self.call(Operation::SetCentralGuard{action})?{Payload::CentralPlugin(value)=>Ok(value),_=>Err(error("Unexpected plugin response"))}}
    fn logo_call(&self,operation:Operation)->Result<serde_json::Value,DeviceError>{match self.call(operation)?{Payload::BootLogo(value)=>Ok(value),_=>Err(error("Unexpected boot logo response"))}}
    pub fn get_boot_logo(&self)->Result<serde_json::Value,DeviceError>{self.logo_call(Operation::BootLogo)}
    pub fn prepare_boot_logo_image(&self,source:std::path::PathBuf)->Result<serde_json::Value,DeviceError>{self.logo_call(Operation::PrepareBootLogo(boot_logo::BootLogoAction::Image{source}))}
    pub fn prepare_boot_logo_reset(&self)->Result<serde_json::Value,DeviceError>{self.logo_call(Operation::PrepareBootLogo(boot_logo::BootLogoAction::FactoryReset{}))}
    pub fn prepare_boot_logo_restore(&self,backup_id:String)->Result<serde_json::Value,DeviceError>{self.logo_call(Operation::PrepareBootLogo(boot_logo::BootLogoAction::Restore{backup_id}))}
    pub fn commit_boot_logo(&self,plan_id:String)->Result<serde_json::Value,DeviceError>{self.logo_call(Operation::CommitBootLogo{plan_id})}
    pub fn discard_boot_logo(&self,plan_id:String)->Result<serde_json::Value,DeviceError>{self.logo_call(Operation::DiscardBootLogo{plan_id})}
    pub fn get_audio(&self)->Result<serde_json::Value,DeviceError>{match self.call(Operation::Audio)?{Payload::Audio(value)=>Ok(value),_=>Err(error("Unexpected audio response"))}}
    pub fn set_audio(&self,preset:audio_controls::AudioPreset)->Result<serde_json::Value,DeviceError>{match self.call(Operation::SetAudio{preset})?{Payload::Audio(value)=>Ok(value),_=>Err(error("Unexpected audio response"))}}
    pub fn get_usb_charge(&self)->Result<serde_json::Value,DeviceError> {match self.call(Operation::UsbCharge)?{Payload::UsbCharge(value)=>Ok(value),_=>Err(error("Unexpected USB response"))}}
    pub fn set_usb_charge(&self,state:UsbState)->Result<serde_json::Value,DeviceError> {match self.call(Operation::SetUsbCharge(state))?{Payload::UsbCharge(value)=>Ok(value),_=>Err(error("Unexpected USB response"))}}
    pub fn set_vendor_control(&self,setting:VendorSetting,enabled:bool)->Result<serde_json::Value,DeviceError>{match self.call(Operation::SetVendorControl{setting,enabled})?{Payload::VendorControls(value)=>Ok(value),_=>Err(error("Unexpected vendor response"))}}
    pub fn vendor_controls(&self)->Result<serde_json::Value,DeviceError>{match self.call(Operation::VendorControls)?{Payload::VendorControls(value)=>Ok(value),_=>Err(error("Unexpected vendor response"))}}
    pub fn keep_alive(&self)->Result<(),DeviceError>{match self.call(Operation::Ping)?{Payload::Pong=>Ok(()),_=>Err(error("Unexpected heartbeat response"))}}
    /// Local process check only. A live helper may still have an unavailable pipe/device.
    /// Never launches a process, prompts for elevation, or performs hardware I/O.
    pub fn is_alive(&self) -> Result<bool, DeviceError> {
        #[cfg(windows)] { platform::is_alive(&self.session) }
        #[cfg(not(windows))] { Err(error("Broker requires Windows")) }
    }
    /// Call only from a user-initiated native command on a blocking thread.
    pub fn launch_user_initiated() -> Result<Self, DeviceError> {
        #[cfg(windows)] { Ok(Self { session: platform::launch()?, next_id: AtomicU64::new(1), call_lock: std::sync::Mutex::new(()) }) }
        #[cfg(not(windows))] { Err(error("Broker requires Windows")) }
    }
    fn call(&self, operation: Operation) -> Result<Payload, DeviceError> {
        let _call_guard = self.call_lock.lock().map_err(error)?;
        let id = self.next_id.try_update(Ordering::Relaxed, Ordering::Relaxed, |id|id.checked_add(1)).map_err(|_|error("Request ID exhausted; reconnect explicitly"))?;
        let request = Request { version: VERSION, id, operation };
        #[cfg(windows)] {
            let response: Response = serde_json::from_slice(&platform::exchange(&self.session, &serde_json::to_vec(&request).map_err(error)?,std::time::Duration::from_secs(request.operation.budget_seconds()+5))?).map_err(error)?;
            if response.version != VERSION || response.id != id { return Err(error("RPC response identity mismatch")); }
            response.result
        }
        #[cfg(not(windows))] { let _ = request; Err(error("Broker requires Windows")) }
    }
    pub fn snapshot(&self) -> Result<Snapshot, DeviceError> { match self.call(Operation::Snapshot)? { Payload::Snapshot(v) => Ok(v), _ => Err(error("Unexpected response type")) } }
    pub fn capabilities(&self) -> Result<Capabilities, DeviceError> { match self.call(Operation::Capabilities)? { Payload::Capabilities(v) => Ok(v), _ => Err(error("Unexpected response type")) } }
    pub fn apply(&self, command: Command) -> Result<ApplyReport, DeviceError> { match self.call(Operation::Apply(command))? { Payload::Apply(v) => Ok(v), _ => Err(error("Unexpected response type")) } }
    /// Uses the already-authorized helper; never launches or prompts for elevation.
    /// A task operation failure is returned in the JSON `error` field alongside readback.
    pub fn official_task(&self, action: OfficialAction) -> Result<serde_json::Value, DeviceError> {
        match self.call(Operation::OfficialTask(action))? { Payload::OfficialTask(value)=>Ok(value),_=>Err(error("Unexpected official task response")) }
    }
}
pub fn run_helper() -> Result<(), DeviceError> {
    #[cfg(windows)] { platform::serve() }
    #[cfg(not(windows))] { Err(error("Broker requires Windows")) }
}
#[cfg(windows)] mod platform;
mod official;
mod vendor_controls;
mod usb_controls;
mod direct_controls;
pub mod audio_controls;
mod audio_bridge;
mod boot_logo;
pub use acer_controls::{UsbState,UsbThreshold};
pub use vendor_controls::VendorSetting;
/// Read-only official service query; no elevation or hardware writes.
pub fn query_vendor_controls()->serde_json::Value{vendor_controls::query(false)}
/// Explicit local diagnostic roundtrip. This is not exposed as a desktop RPC.
pub fn validate_vendor_control(setting:VendorSetting)->Result<serde_json::Value,DeviceError>{
    let capabilities=acer_device::Device::connect()?.capabilities()?;
    if !acer_device::verified_fan_target(&capabilities.model,&capabilities.bios){return Err(error("Vendor validation requires PHN16-71 / V1.18"))}
    vendor_controls::validate(setting)
}

#[cfg(test)] mod tests {
    use super::*;
    #[test] fn thermal_rpc_exposes_only_curves_and_fixed_lifecycle_operations() {
        for value in [r#"{"op":"thermal_status"}"#,r#"{"op":"stop_thermal"}"#] {
            assert!(serde_json::from_str::<Operation>(value).is_ok());
        }
        let curve=serde_json::json!({"points":[{"celsius":40.0,"percent":50},{"celsius":80.0,"percent":100}],"hysteresis_celsius":3.0,"minimum_write_interval_ms":1000,"maximum_sample_age_ms":5000});
        let mut value=serde_json::json!({"op":"start_thermal","command":{"cpu":curve.clone(),"gpu":curve}});
        assert!(serde_json::from_value::<Operation>(value.clone()).is_ok());
        value["command"]["method"]=serde_json::json!("SetGamingFanTable");
        assert!(serde_json::from_value::<Operation>(value).is_err());
        for value in [r#"{"op":"stop_thermal","policy":"verified"}"#,r#"{"op":"thermal_status","path":"arbitrary"}"#] {
            assert!(serde_json::from_str::<Operation>(value).is_err());
        }
    }
    #[test]fn plugin_operations_reject_arbitrary_task_and_restart(){
        assert!(serde_json::from_str::<Operation>(r#"{"op":"set_central_guard","command":{"action":"disable"}}"#).is_ok());
        for json in [r#"{"op":"set_central_guard","command":{"action":"restart"}}"#,r#"{"op":"set_central_guard","command":{"action":"enable","task":"Other"}}"#,r#"{"op":"central_plugin","path":"C:\\other"}"#]{assert!(serde_json::from_str::<Operation>(json).is_err());}
    }
    #[test]fn core_release_rejects_removed_calibration_operations(){
        for json in [r#"{"op":"calibration"}"#,r#"{"op":"start_calibration"}"#,r#"{"op":"set_calibration","command":{"action":"start"}}"#,r#"{"op":"set_calibration","command":{"action":"recover","session_id":"123-1"}}"#]{assert!(serde_json::from_str::<Operation>(json).is_err());}
    }
    #[test] fn logo_rpc_has_fixed_operations_and_native_budgets(){
        let prepare:Operation=serde_json::from_str(r#"{"op":"prepare_boot_logo","command":{"kind":"factory_reset"}}"#).unwrap();
        assert_eq!(prepare.budget_seconds(),120);
        let commit:Operation=serde_json::from_str(r#"{"op":"commit_boot_logo","command":{"plan_id":"123-45-1"}}"#).unwrap();
        assert_eq!(commit.budget_seconds(),120);assert_eq!(Operation::Snapshot.budget_seconds(),15);
        for value in [
            r#"{"op":"prepare_boot_logo","command":{"kind":"factory_reset","targets":["C:\\other"]}}"#,
            r#"{"op":"commit_boot_logo","command":{"plan_id":"id","manifest":{}}}"#,
            r#"{"op":"commit_boot_logo","command":{"plan_id":"id","timeout":999}}"#,
            r#"{"op":"boot_logo","targets":[]}"#,
        ]{assert!(serde_json::from_str::<Operation>(value).is_err(),"accepted {value}");}
    }
    #[test] fn audio_rpc_accepts_only_fixed_presets_without_transport_overrides(){
        assert!(serde_json::from_str::<Operation>(r#"{"op":"audio"}"#).is_ok());
        for preset in audio_controls::AudioPreset::ALL {
            let op=Operation::SetAudio{preset};
            assert!(serde_json::from_slice::<Operation>(&serde_json::to_vec(&op).unwrap()).is_ok());
        }
        for value in [
            r#"{"op":"set_audio","command":{"preset":9}}"#,
            r#"{"op":"set_audio","command":{"preset":"arbitrary"}}"#,
            r#"{"op":"set_audio","command":{"preset":"music","policy":"supervised_validation"}}"#,
            r#"{"op":"set_audio","command":{"preset":"music","Function":"ARBITRARY"}}"#,
            r#"{"op":"audio","host":"example.com"}"#,
        ]{assert!(serde_json::from_str::<Operation>(value).is_err(),"accepted {value}");}
    }
    #[test] fn usb_rpc_accepts_only_fixed_bool_and_threshold_pair(){
        assert!(serde_json::from_str::<Operation>(r#"{"op":"usb_charge"}"#).is_ok());
        for threshold in [10,20,30] {
            let json=format!(r#"{{"op":"set_usb_charge","command":{{"enabled":true,"threshold":{threshold}}}}}"#);
            assert!(serde_json::from_str::<Operation>(&json).is_ok());
        }
        for value in [
            r#"{"op":"set_usb_charge","command":{"enabled":true,"threshold":25}}"#,
            r#"{"op":"set_usb_charge","command":{"enabled":true,"threshold":"30"}}"#,
            r#"{"op":"set_usb_charge","command":{"enabled":1,"threshold":30}}"#,
            r#"{"op":"set_usb_charge","command":{"enabled":true,"threshold":30,"policy":"verified"}}"#,
            r#"{"op":"set_usb_charge","command":{"enabled":true,"threshold":30,"uiInput":1969924}}"#,
            r#"{"op":"set_usb_charge","command":{"enabled":true,"threshold":30,"method":"SetFunction"}}"#,
            r#"{"op":"usb_charge","path":"C:\\arbitrary"}"#,
        ] {assert!(serde_json::from_str::<Operation>(value).is_err(),"accepted {value}");}
    }
    #[test] fn vendor_rpc_is_typed_and_rejects_extra_protocol_parameters(){
        assert!(serde_json::from_str::<Operation>(r#"{"op":"set_vendor_control","command":{"setting":"boot_sound","enabled":true}}"#).is_ok());
        for value in [
            r#"{"op":"set_vendor_control","command":{"setting":"arbitrary","enabled":true}}"#,
            r#"{"op":"set_vendor_control","command":{"setting":"boot_sound","enabled":1}}"#,
            r#"{"op":"set_vendor_control","command":{"setting":"boot_sound","enabled":true,"Function":"ARBITRARY"}}"#,
            r#"{"op":"vendor_controls","host":"example.com"}"#,
        ]{assert!(serde_json::from_str::<Operation>(value).is_err(),"accepted {value}");}
    }
    #[test] fn rpc_rejects_arbitrary_operations_and_policy() {
        for operation in [r#"{"op":"invoke","command":"SetGamingProfile"}"#, r#"{"op":"snapshot","allow_unverified":true}"#] {
            assert!(serde_json::from_str::<Operation>(operation).is_err());
        }
        assert!(serde_json::from_str::<Request>(r#"{"version":1,"id":1,"operation":{"op":"snapshot"},"path":"cmd.exe"}"#).is_err());
    }
    #[test] fn request_roundtrip() {
        let value = Request { version: VERSION, id: 42, operation: Operation::Snapshot };
        let decoded: Request = serde_json::from_slice(&serde_json::to_vec(&value).unwrap()).unwrap();
        assert_eq!(decoded.id, 42);
    }
    #[test] fn official_actions_are_fixed_and_roundtrip() {
        for action in [OfficialAction::Query,OfficialAction::Backup,OfficialAction::Disable,OfficialAction::Restore] {
            let value=Request {version:VERSION,id:9,operation:Operation::OfficialTask(action)};
            let encoded=serde_json::to_vec(&value).unwrap();
            let decoded:Request=serde_json::from_slice(&encoded).unwrap();
            assert!(matches!(decoded.operation,Operation::OfficialTask(a) if a==action));
        }
    }
    #[test] fn official_rpc_rejects_unknown_actions_paths_and_commands() {
        for json in [
            r#"{"op":"official_task","command":"delete"}"#,
            r#"{"op":"official_task","command":{"action":"disable","path":"C:\\Windows"}}"#,
            r#"{"op":"official_task","command":"query","path":"C:\\Windows"}"#,
            r#"{"op":"official_task","command":"disable","task_name":"OtherTask"}"#,
            r#"{"op":"official_task","command":"query","arguments":"/DELETE"}"#,
        ] {assert!(serde_json::from_str::<Operation>(json).is_err(),"accepted {json}");}
    }
}
