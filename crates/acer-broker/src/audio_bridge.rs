use super::*;
use super::audio_controls::{AudioClient, AudioPreset, AudioState, WritePolicy};
use serde_json::{json,Value};
use std::sync::{Arc,Mutex};

#[derive(Default)]
pub(crate) struct AudioControls { client:Mutex<Option<Arc<AudioClient>>> }
impl AudioControls {
    fn client(&self,device:&acer_device::Device)->Result<Arc<AudioClient>,DeviceError> {
        let mut slot=self.client.lock().map_err(error)?;
        if slot.is_none(){*slot=Some(Arc::new(AudioClient::for_device(device,WritePolicy::ExplicitApply)?));}
        Ok(slot.as_ref().unwrap().clone())
    }
    pub(crate) fn query(&self,device:&acer_device::Device)->Result<Value,DeviceError> {
        let query=self.client(device)?.query()?;
        let (availability,preset)=match query.state.value {
            Some(AudioState::Available{preset})=>("available",Some(preset)),
            Some(AudioState::OutputUnavailable{..})=>("output_unavailable",None),
            None=>("unknown",None),
        };
        let message=query.capability.error.as_ref().or(query.state.error.as_ref()).map(|e|e.message.as_str());
        Ok(json!({"backend":"official_service","dependency":"使用现有 Acer 音效服务与 DTS 音效组件。",
            "supported":query.capability.value.as_ref().map(|c|c.supported.as_slice()).unwrap_or(&[]),
            "preset":preset,"availability":availability,
            "writable":query.writable&&query.setter_ack_schema_verified,"error":message}))
    }
    pub(crate) fn apply(&self,device:&acer_device::Device,preset:AudioPreset)->Result<Value,DeviceError> {
        let report=self.client(device)?.set(preset)?;
        let message=match report.status {
            ApplyStatus::Applied=>"音效已应用，服务读回一致",
            ApplyStatus::Rejected=>"未修改音效：请检查当前输出设备与支持模式",
            ApplyStatus::Failed=>"音效服务拒绝设置，请刷新检查当前状态",
            _=>"音效结果尚未确认，请刷新检查当前状态",
        };
        Ok(json!({"status":report.status,"message":message,"report":report}))
    }
}
