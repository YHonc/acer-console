//! Fixed target-guarded WMI controls. No official-service fallback or raw requests.
use super::*;
use acer_controls::{direct_settings::{DirectSettingsDevice, Setting, WritePolicy}, Ack};
use serde_json::{json, Value};
use std::sync::Mutex;

#[derive(Default)]
pub(crate) struct DirectControls { device: Mutex<Option<DirectSettingsDevice>> }
impl DirectControls {
    fn device(&self) -> Result<DirectSettingsDevice,DeviceError> {
        let mut slot=self.device.lock().map_err(error)?;
        if slot.is_none() { *slot=Some(DirectSettingsDevice::connect_with_policy(WritePolicy::Verified)?); }
        Ok(slot.as_ref().unwrap().clone())
    }
    pub(crate) fn query(&self) -> Value {
        let report=self.device().and_then(|device|device.read_all());
        let items:Vec<Value>=match report {
            Ok(report)=>report.readings.into_iter().map(|read|{
                let writable=read.ack==Ack::Confirmed && read.state.is_some();
                let message=if writable {None}else{Some(read.raw.error.as_ref().map(|e|e.message.clone()).unwrap_or_else(||"设备状态尚未确认".into()))};
                json!({"setting":read.setting,"enabled":read.state.map(|s|s.enabled()),"writable":writable,"error":message,"raw":read,"backend":"direct_wmi"})
            }).collect(),
            Err(e)=>Setting::ALL.into_iter().map(|setting|json!({"setting":setting,"enabled":null,"writable":false,"error":e.message,"backend":"direct_wmi"})).collect(),
        };
        json!({"backend":"direct_wmi","dependency":"三项硬件设置通过本机 WMI 直接控制。","items":items})
    }
    pub(crate) fn apply(&self,setting:VendorSetting,enabled:bool)->Result<Value,DeviceError> {
        let setting=match setting {
            VendorSetting::LcdOverdrive=>Setting::LcdOverdrive,
            VendorSetting::BootSound=>Setting::BootSound,
            VendorSetting::BacklightTimeout=>Setting::BacklightTimeout,
            VendorSetting::StickyKey=>return Err(error("StickyKeys belongs to the interactive desktop user")),
        };
        let report=self.device()?.set(setting,enabled)?;
        let message=match report.status { ApplyStatus::Applied=>"设置已应用，硬件读回一致",ApplyStatus::Rejected=>"未写入：原始状态或权限尚未确认",ApplyStatus::Failed=>"硬件设置失败，请刷新检查当前状态",_=>"写入结果尚未确认，请刷新检查当前状态" };
        Ok(json!({"status":report.status,"setting":setting,"enabled":report.after.as_ref().and_then(|r|r.state).map(|s|s.enabled()),"message":message,"report":report,"backend":"direct_wmi"}))
    }
}
