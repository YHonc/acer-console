//! Lazy target-guarded USB worker; no polling, automatic elevation, or vendor-cache sync.
use super::*;
use acer_controls::{Ack,UsbApplyReport,UsbDevice,UsbRead,WritePolicy};
use serde_json::{json,Value};
use std::sync::Mutex;

#[derive(Default)]
pub(crate) struct UsbControls {device:Mutex<Option<UsbDevice>>}
impl UsbControls {
    fn device(&self)->Result<UsbDevice,DeviceError> {
        let mut slot=self.device.lock().map_err(error)?;
        if slot.is_none(){*slot=Some(UsbDevice::connect_with_policy(WritePolicy::Verified)?);}
        Ok(slot.as_ref().unwrap().clone())
    }
    pub(crate) fn query(&self)->Value {
        match self.device().and_then(|device|device.get()) {
            Ok(read)=>query_response(read),
            Err(err)=>json!({"state":null,"writable":false,"ack":"unknown","error":err.message,"device_error":err,"backend":"direct_wmi","raw":null}),
        }
    }
    pub(crate) fn set(&self,state:UsbState)->Result<Value,DeviceError> {
        let report=self.device()?.set(state)?;
        Ok(set_response(report))
    }
}
fn query_response(read:UsbRead)->Value {
    let writable=read.ack==Ack::Confirmed&&read.state.is_some();
    let message=if writable {None}else{Some(read.raw.error.as_ref().map(|e|e.to_string()).unwrap_or_else(||"USB 开关和阈值未被可靠确认，暂不可修改".into()))};
    json!({"state":read.state,"writable":writable,"ack":read.ack,"error":message,"backend":"direct_wmi","decoded_state":read.decoded_state,"raw":read.raw})
}
fn set_response(report:UsbApplyReport)->Value {
    let(status,message)=match report.status {
        ApplyStatus::Applied=>("applied","USB 关机充电设置已应用，开关和阈值读回已确认"),
        ApplyStatus::Rejected=>("failed","当前 USB 原状态或权限条件未确认，未修改设置"),
        ApplyStatus::Failed=>("failed","USB 设置失败，请读取当前状态并检查诊断记录"),
        ApplyStatus::Unconfirmed=>("unconfirmed","请求已发送，但 USB 设置尚未确认，请刷新检查"),
    };
    let state=report.after.as_ref().and_then(|read|read.state);
    json!({"status":status,"message":message,"state":state,"backend":"direct_wmi","diagnostic":report})
}

#[cfg(test)] mod tests {
    use super::*;
    use std::collections::BTreeMap;
    fn reading()->UsbRead {
        let state=UsbState{enabled:true,threshold:UsbThreshold::Percent30};
        UsbRead{state:Some(state),decoded_state:Some(state),ack:Ack::Confirmed,raw:RawRead{class:"APGeAction".into(),method:"GetFunction".into(),input:BTreeMap::from([("uiInput".into(),RawValue::unsigned(4))]),output:BTreeMap::from([("uiOutput".into(),RawValue::unsigned(1969920))]),error:None}}
    }
    #[test] fn query_requires_confirmed_pair_before_enabling_writes() {
        let value=query_response(reading());assert_eq!(value["writable"],true);assert_eq!(value["backend"],"direct_wmi");assert!(value["error"].is_null());
        let mut read=reading();read.state=None;read.ack=Ack::Unknown;
        let value=query_response(read);assert_eq!(value["writable"],false);assert!(value["state"].is_null());assert!(value["error"].is_string());
    }
    #[test] fn matching_state_alone_does_not_promote_unconfirmed_report() {
        let read=reading();let state=read.state.unwrap();
        let value=set_response(UsbApplyReport{status:ApplyStatus::Unconfirmed,message:"fixture unknown ACK".into(),target:state,before:None,writes:vec![],after:Some(read)});
        assert_eq!(value["status"],"unconfirmed");assert_eq!(value["diagnostic"]["status"],"unconfirmed");
    }
}
