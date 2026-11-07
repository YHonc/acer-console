use super::*;
use std::collections::VecDeque;
fn state(enabled:bool)->UsbState {UsbState{enabled,threshold:UsbThreshold::Percent20}}
fn get(state:UsbState)->RawRead {raw_read("GetFunction",4,BTreeMap::from([("ReturnValue".into(),RawValue::Bool(true)),("uiOutput".into(),RawValue::unsigned(encode_usb(state)&!255))]),None)}
fn failed_get()->RawRead {raw_read("GetFunction",4,BTreeMap::new(),Some(error(ErrorKind::PermissionDenied,"fixture read failure")))}
fn write()->WriteAttempt {WriteAttempt{attempted:true,raw:raw_read("SetFunction",encode_usb(state(false)),BTreeMap::from([("ReturnValue".into(),RawValue::Bool(true)),("uiOutput".into(),RawValue::unsigned(0))]),None)}}
struct Fake {reads:VecDeque<RawRead>,writes:VecDeque<WriteAttempt>,sent:Vec<UsbState>}
impl Transport for Fake {
    fn read(&mut self)->RawRead {self.reads.pop_front().expect("unexpected GET")}
    fn write(&mut self,target:UsbState)->WriteAttempt {self.sent.push(target);self.writes.pop_front().expect("unexpected SET")}
}
fn controller(reads:Vec<RawRead>,writes:Vec<WriteAttempt>)->Controller<Fake> {Controller{transport:Fake{reads:reads.into(),writes:writes.into(),sent:vec![]},policy:WritePolicy::SupervisedValidation}}
#[test] fn exact_target_and_codec() {
    assert!(matches_target("Predator PHN16-71","V1.18"));assert!(!matches_target("PHN16-72","V1.18"));assert!(!matches_target("PHN16-71","V1.19"));
    for threshold in [UsbThreshold::Percent10,UsbThreshold::Percent20,UsbThreshold::Percent30] {for enabled in [false,true] {
        let s=UsbState{enabled,threshold};assert_eq!(decode_usb(encode_usb(s)&!255),Some(s));
    }}
    assert_eq!(encode_usb(UsbState{enabled:true,threshold:UsbThreshold::Percent10}),659204);
    assert_eq!(encode_usb(UsbState{enabled:false,threshold:UsbThreshold::Percent10}),663300);
    for raw in [0,0x140e00,0x190f00,0x140f01,0x100140f00] {assert!(decode_usb(raw).is_none());}
    assert!(serde_json::from_str::<UsbState>(r#"{"enabled":true,"threshold":25}"#).is_err());
}
#[test] fn unreviewed_missing_return_and_numeric_return_do_not_succeed() {
    let mut raw=get(state(true));raw.output.remove("ReturnValue");raw.input.insert("uiInput".into(),RawValue::unsigned(5));let reading=UsbRead::from_raw(raw.clone());
    assert_eq!(reading.ack,Ack::Unknown);assert_eq!(reading.state,None);assert_eq!(reading.decoded_state,Some(state(true)));
    raw.output.insert("ReturnValue".into(),RawValue::unsigned(0));assert_eq!(ack(&raw),Ack::Rejected);
    raw.output.insert("ReturnValue".into(),RawValue::Bool(false));assert_eq!(ack(&raw),Ack::Rejected);
}
#[test] fn unknown_get_and_readonly_policy_never_write() {
    let mut c=controller(vec![failed_get()],vec![]);let report=c.set(state(false));assert_eq!(report.status,ApplyStatus::Rejected);assert!(c.transport.sent.is_empty());
    c.policy=WritePolicy::ReadOnly;assert_eq!(c.set(state(false)).status,ApplyStatus::Rejected);
    let mut unknown=get(state(true));unknown.output.remove("ReturnValue");unknown.input.insert("uiInput".into(),RawValue::unsigned(5));let mut c=controller(vec![unknown],vec![]);
    let result=c.validate();assert!(result.trial.is_none());assert!(!result.restore_required);assert!(c.transport.sent.is_empty());
}
fn real_get()->RawRead {
    let fixture:serde_json::Value=serde_json::from_str(include_str!("../tests/fixtures/phn16-71-usb-get.json")).unwrap();
    serde_json::from_value(fixture["raw"].clone()).unwrap()
}
#[test] fn reviewed_real_get_missing_return_decodes_enabled_thirty() {
    let raw=real_get();assert!(!raw.output.contains_key("ReturnValue"));
    let reading=UsbRead::from_raw(raw);
    assert_eq!(reading.ack,Ack::Confirmed);assert_eq!(reading.state,Some(UsbState{enabled:true,threshold:UsbThreshold::Percent30}));
}
#[test] fn reviewed_get_exception_does_not_hide_explicit_failure_or_other_schema() {
    for value in [RawValue::Bool(false),RawValue::unsigned(0),RawValue::Null,RawValue::Text("true".into())] {
        let mut raw=real_get();raw.output.insert("ReturnValue".into(),value);assert_eq!(UsbRead::from_raw(raw).ack,Ack::Rejected);
    }
    let mut variants=vec![];
    let mut raw=real_get();raw.class="AcerGamingFunction".into();variants.push(raw);
    let mut raw=real_get();raw.method="SetFunction".into();variants.push(raw);
    let mut raw=real_get();raw.input.insert("extra".into(),RawValue::unsigned(0));variants.push(raw);
    let mut raw=real_get();raw.output.insert("extra".into(),RawValue::unsigned(0));variants.push(raw);
    let mut raw=real_get();raw.output.insert("uiOutput".into(),RawValue::unsigned(0x1e0e00));variants.push(raw);
    let mut raw=real_get();raw.output.insert("uiOutput".into(),RawValue::Text("1969920".into()));variants.push(raw);
    for raw in variants {assert_eq!(UsbRead::from_raw(raw).state,None);}
    let mut raw=real_get();raw.output.insert("uiOutput".into(),RawValue::unsigned(0x1e0f01));assert_eq!(ack(&raw),Ack::Rejected);
    let mut raw=real_get();raw.error=Some(error(ErrorKind::Transport,"fixture"));assert_eq!(ack(&raw),Ack::Rejected);
}
#[test] fn supervised_unknown_setter_ack_still_reads_and_restores_original_pair() {
    let original=real_get();let saved=UsbRead::from_raw(original.clone()).state.unwrap();
    let target=UsbState{enabled:false,..saved};let mut target_read=original.clone();
    target_read.output.insert("uiOutput".into(),RawValue::unsigned(encode_usb(target)&!255));
    let mut unknown_write=write();unknown_write.raw.output.remove("ReturnValue");unknown_write.raw.output.insert("unreviewed".into(),RawValue::unsigned(0));
    let mut c=controller(vec![original.clone(),target_read,original],vec![unknown_write.clone(),unknown_write]);
    let report=c.validate();
    assert_eq!(report.trial.as_ref().unwrap().status,ApplyStatus::Unconfirmed);
    assert_eq!(report.trial.as_ref().unwrap().after.as_ref().unwrap().state,Some(target));
    assert_eq!(report.restore.as_ref().unwrap().status,ApplyStatus::Unconfirmed);
    assert_eq!(report.restore.as_ref().unwrap().after.as_ref().unwrap().state,Some(saved));
    assert!(report.restore_required);assert!(!report.restored);assert_eq!(c.transport.sent,vec![target,saved]);
}
#[test] fn set_requires_ack_and_full_pair_independent_readback() {
    let mut c=controller(vec![get(state(true)),get(state(false))],vec![write()]);assert_eq!(c.set(state(false)).status,ApplyStatus::Applied);
    let wrong=UsbState{enabled:false,threshold:UsbThreshold::Percent10};
    let mut c=controller(vec![get(state(true)),get(wrong)],vec![write()]);assert_eq!(c.set(state(false)).status,ApplyStatus::Unconfirmed);
    let mut missing=write();missing.raw.output.remove("ReturnValue");missing.raw.output.insert("unreviewed".into(),RawValue::unsigned(0));
    let mut c=controller(vec![get(state(true)),get(state(false))],vec![missing]);assert_eq!(c.set(state(false)).status,ApplyStatus::Unconfirmed);
}
#[test] fn validation_restores_even_when_trial_readback_fails() {
    let mut c=controller(vec![get(state(true)),failed_get(),get(state(true))],vec![write(),write()]);
    let result=c.validate();assert_eq!(result.trial.unwrap().status,ApplyStatus::Unconfirmed);assert!(result.restored);assert!(!result.restore_required);
    assert_eq!(c.transport.sent,vec![state(false),state(true)]);assert!(c.transport.reads.is_empty());
}
#[test] fn attempted_set_error_still_restores_and_failed_restore_stays_pending() {
    let failed=WriteAttempt{attempted:true,raw:raw_read("SetFunction",encode_usb(state(false)),BTreeMap::new(),Some(error(ErrorKind::Transport,"fixture exec failure")))};
    let mut restore=write();restore.raw.output.remove("ReturnValue");restore.raw.output.insert("unreviewed".into(),RawValue::unsigned(0));
    let mut c=controller(vec![get(state(true)),failed_get(),get(state(true))],vec![failed,restore]);
    let result=c.validate();assert_eq!(result.trial.unwrap().status,ApplyStatus::Failed);assert!(result.restore_required);assert!(!result.restored);assert_eq!(c.transport.sent,vec![state(false),state(true)]);
}
#[test] fn preparation_failure_does_not_claim_dispatched_or_restore() {
    let failed=WriteAttempt{attempted:false,raw:raw_read("SetFunction",encode_usb(state(false)),BTreeMap::new(),Some(error(ErrorKind::PermissionDenied,"fixture instance denied")))};
    let mut c=controller(vec![get(state(true))],vec![failed]);let report=c.validate();
    assert!(report.restore.is_none());assert!(!report.trial.unwrap().write_attempted());assert!(!report.restore_required);
}
fn real_roundtrip()->(RawRead,RawRead,RawRead,WriteAttempt,WriteAttempt) {
    let fixture:serde_json::Value=serde_json::from_str(include_str!("../tests/fixtures/phn16-71-usb-switch-restore.json").trim_start_matches('\u{feff}')).unwrap();
    let raw=|value:&serde_json::Value|serde_json::from_value::<RawRead>(value.clone()).unwrap();
    (raw(&fixture["original"]["raw"]),raw(&fixture["trial"]["after"]["raw"]),raw(&fixture["restore"]["after"]["raw"]),
        WriteAttempt{attempted:true,raw:raw(&fixture["trial"]["writes"][0]["raw"])},WriteAttempt{attempted:true,raw:raw(&fixture["restore"]["writes"][0]["raw"])})
}
#[test] fn real_switch_fixture_confirms_exact_setter_and_original_pair_restore() {
    let (original,trial_after,restore_after,trial,restore)=real_roundtrip();
    assert!(!trial.raw.output.contains_key("ReturnValue"));assert_eq!(ack(&trial.raw),Ack::Confirmed);assert_eq!(ack(&restore.raw),Ack::Confirmed);
    let mut c=controller(vec![original,trial_after,restore_after],vec![trial,restore]);let report=c.validate();
    assert_eq!(report.trial.unwrap().status,ApplyStatus::Applied);assert_eq!(report.restore.unwrap().status,ApplyStatus::Applied);
    assert!(report.restored);assert!(!report.restore_required);
    assert_eq!(c.transport.sent,vec![UsbState{enabled:false,threshold:UsbThreshold::Percent30},UsbState{enabled:true,threshold:UsbThreshold::Percent30}]);
}
#[test] fn setter_missing_return_exception_rejects_failures_and_nonzero_ack() {
    let (_,_,_,write,_)=real_roundtrip();
    for value in [RawValue::Bool(false),RawValue::Null,RawValue::unsigned(0),RawValue::Text("true".into())] {
        let mut raw=write.raw.clone();raw.output.insert("ReturnValue".into(),value);assert_eq!(ack(&raw),Ack::Rejected);
    }
    for value in [1,0x100,u32::MAX as u64] {let mut raw=write.raw.clone();raw.output.insert("uiOutput".into(),RawValue::unsigned(value));assert_eq!(ack(&raw),Ack::Rejected);}
    for value in [0,4,0x1e1e04,0x191f04,0x1e1f05,0x1001e1f04] {
        let mut raw=write.raw.clone();raw.input.insert("uiInput".into(),RawValue::unsigned(value));assert_eq!(ack(&raw),Ack::Unknown);
    }
    let mut raw=write.raw.clone();raw.class="OtherClass".into();assert_eq!(ack(&raw),Ack::Unknown);
    let mut raw=write.raw.clone();raw.output.insert("uiOutput".into(),RawValue::Text("0".into()));assert_eq!(ack(&raw),Ack::Unknown);
    let mut raw=write.raw;raw.error=Some(error(ErrorKind::Transport,"fixture error"));assert_eq!(ack(&raw),Ack::Rejected);
}
#[test] fn verified_policy_keeps_initial_get_and_full_readback_guards() {
    let (original,after,_,write,_)=real_roundtrip();let target=UsbRead::from_raw(after.clone()).state.unwrap();
    let mut c=controller(vec![original,after],vec![write]);c.policy=WritePolicy::Verified;assert_eq!(c.set(target).status,ApplyStatus::Applied);
    let mut c=controller(vec![failed_get()],vec![]);c.policy=WritePolicy::Verified;assert_eq!(c.set(target).status,ApplyStatus::Rejected);assert!(c.transport.sent.is_empty());
}
#[test] fn threshold_validation_preserves_enabled_and_restores_even_on_read_failure() {
    for enabled in [false,true] {
        let saved=UsbState{enabled,threshold:UsbThreshold::Percent30};let mut original=get(saved);original.output.remove("ReturnValue");
        let mut c=controller(vec![original.clone(),failed_get(),original],vec![write(),write()]);
        let report=c.validate_threshold(UsbThreshold::Percent10);assert!(report.restored);assert!(!report.restore_required);
        assert_eq!(c.transport.sent,vec![UsbState{enabled,threshold:UsbThreshold::Percent10},saved]);
    }
    let mut c=controller(vec![get(state(true))],vec![]);let report=c.validate_threshold(UsbThreshold::Percent20);assert!(report.trial.is_none());assert!(c.transport.sent.is_empty());
}
