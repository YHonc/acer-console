use super::*;
use std::collections::VecDeque;

struct Step { call:Call, write:bool, response:WriteAttempt }
struct Fake { steps:VecDeque<Step>, calls:Vec<(Call,bool)> }
impl Fake { fn new(steps:Vec<Step>) -> Self { Self { steps:steps.into(),calls:vec![] } } }
impl Transport for Fake {
    fn call(&mut self,call:Call,write:bool) -> WriteAttempt {
        let step=self.steps.pop_front().expect("unexpected extra WMI request");
        assert_eq!((call,write),(step.call,step.write));self.calls.push((call,write));step.response
    }
}
fn identity() -> Identity { Identity { model:"Predator PHN16-71".into(),bios:"V1.18".into() } }
fn controller(steps:Vec<Step>) -> Controller<Fake> { Controller::new(Fake::new(steps),WritePolicy::SupervisedValidation,identity()).unwrap() }
fn sample(setting:Setting,enabled:bool,brightness:u8) -> State { match setting {
    Setting::LcdOverdrive=>State::LcdOverdrive{enabled},Setting::BootSound=>State::BootSound{enabled},
    Setting::BacklightTimeout=>State::BacklightTimeout{enabled,brightness},
} }
fn get_word(state:State) -> u64 { match state {
    State::LcdOverdrive{enabled}=>u64::from(enabled)<<48,
    State::BootSound{enabled}=>u64::from(enabled)<<8,
    State::BacklightTimeout{enabled,brightness}=>(u64::from(brightness)<<32)|(if enabled{30u64<<40}else{0}),
} }
fn response(call:Call,value:u64,ret:Option<RawValue>) -> RawRead {
    let mut output=BTreeMap::from([(output_field(call).into(),RawValue::unsigned(value))]);
    if let Some(value)=ret { output.insert("ReturnValue".into(),value); }
    call.raw(output,None)
}
fn get(state:State) -> Step {
    let call=get_call(state.setting());Step { call,write:false,response:WriteAttempt { attempted:true,raw:response(call,get_word(state),Some(RawValue::Bool(true))) } }
}
fn set(state:State,ret:Option<RawValue>) -> Step {
    let call=set_call(state);Step { call,write:true,response:WriteAttempt { attempted:true,raw:response(call,0,ret) } }
}
fn acked(state:State) -> Step { set(state,Some(RawValue::Bool(true))) }
fn failed_get(state:State) -> Step {
    let mut step=get(state);step.response.raw.error=Some(error(ErrorKind::Transport,"simulated read failure"));step
}
fn normal_steps(setting:Setting) -> Vec<Step> {
    let original=sample(setting,false,75);let target=original.with_enabled(true);
    let mut steps=vec![get(original),acked(target),get(target)];
    if setting==Setting::BacklightTimeout { steps.push(get(target)); }
    steps.extend([acked(original),get(original)]);steps
}

#[test]
fn exact_native_words_and_brightness_byte_are_preserved() {
    assert_eq!(get_call(Setting::LcdOverdrive).value,0);
    assert_eq!(get_call(Setting::BootSound).value,6);
    assert_eq!(get_call(Setting::BacklightTimeout).value,0x88401);
    assert_eq!(set_call(State::LcdOverdrive{enabled:false}).value,0x10);
    assert_eq!(set_call(State::LcdOverdrive{enabled:true}).value,0x1000000000010);
    assert_eq!(set_call(State::BootSound{enabled:false}).value,6);
    assert_eq!(set_call(State::BootSound{enabled:true}).value,0x106);
    for brightness in [0,1,25,75,100] { for enabled in [false,true] {
        let state=State::BacklightTimeout{enabled,brightness};let word=set_call(state).value;
        assert_eq!((word>>32)&255,u64::from(brightness));
        assert_eq!((word>>40)&255,if enabled{30}else{0});
        assert_eq!(word & 0xffff_ffff,0x88402);
        assert_eq!(decode(Setting::BacklightTimeout,get_word(state)),Some(state));
    } }
    assert_eq!(set_call(State::BacklightTimeout{enabled:false,brightness:100}).value,0x6400088402);
    assert_eq!(set_call(State::BacklightTimeout{enabled:true,brightness:100}).value,0x1e6400088402);
}
#[test]
fn unknown_flags_and_firmware_errors_never_become_false() {
    for flag in [2,30,255] { assert_eq!(decode(Setting::LcdOverdrive,flag<<48),None);assert_eq!(decode(Setting::BootSound,flag<<8),None); }
    for flag in [1,29,31,255] { assert_eq!(decode(Setting::BacklightTimeout,flag<<40),None); }
    for setting in Setting::ALL { assert_eq!(decode(setting,get_word(sample(setting,true,50))|1),None); }
}
#[test]
fn invalid_return_never_authorizes_initial_checkpoint() {
    for setting in Setting::ALL { for ret in [Some(RawValue::Bool(false)),Some(RawValue::unsigned(1))] {
        let state=sample(setting,true,75);let mut step=get(state);
        step.response.raw=response(step.call,get_word(state),ret.clone());
        let mut c=controller(vec![step]);let report=c.validate(setting);
        assert_eq!(report.original.decoded_state,Some(state));assert!(report.original.state.is_none());
        assert_eq!(report.outcome,ValidationOutcome::Rejected);assert!(report.trial.is_none());
        assert_eq!(c.transport.calls.len(),1);assert!(c.transport.steps.is_empty());
    } }
}
#[test]
fn explicit_true_still_requires_unsigned_output_and_exact_selector() {
    let setting=Setting::BootSound;let call=get_call(setting);
    let mut raw=response(call,256,Some(RawValue::Bool(true)));raw.input.insert("gmInput".into(),RawValue::unsigned(11));
    assert_eq!(read_result(setting,raw).ack,Ack::Rejected);
    for value in [RawValue::Signed("256".into()),RawValue::Text("256".into()),RawValue::Null] {
        let mut raw=response(call,256,Some(RawValue::Bool(true)));raw.output.insert("gmOutput".into(),value);
        let read=read_result(setting,raw);assert_eq!(read.ack,Ack::Unknown);assert!(read.state.is_none());
    }
}
#[test]
fn read_only_set_dispatches_nothing_and_validation_only_reads() {
    let mut c=Controller::new(Fake::new(vec![]),WritePolicy::default(),identity()).unwrap();
    assert_eq!(c.set(Setting::BootSound,true).status,ApplyStatus::Rejected);assert!(c.transport.calls.is_empty());
    c.transport.steps.push_back(get(State::BootSound{enabled:false}));
    assert_eq!(c.validate(Setting::BootSound).outcome,ValidationOutcome::Rejected);assert_eq!(c.transport.calls.len(),1);
}
#[test]
fn all_three_success_paths_independently_get_after_trial_and_restore() {
    for setting in Setting::ALL {
        let mut c=controller(normal_steps(setting));let report=c.validate(setting);
        assert!(report.success());assert!(report.target_readback_confirmed&&report.restore_readback_confirmed&&report.original_state_restored&&report.setter_acks_confirmed);
        assert!(!report.restore_required);assert!(c.transport.steps.is_empty());
        assert_eq!(c.transport.calls.iter().filter(|(_,write)|*write).count(),2);
    }
}
#[test]
fn unreviewed_setter_outputs_are_not_promoted_by_matching_gets() {
    for setting in Setting::ALL {
        let mut steps=normal_steps(setting);
        for step in steps.iter_mut().filter(|s|s.write) { step.response.raw.output.remove("ReturnValue");step.response.raw.output.insert("unreviewed".into(),RawValue::unsigned(0)); }
        let mut c=controller(steps);let report=c.validate(setting);
        assert_eq!(report.outcome,ValidationOutcome::ReadbacksConfirmedAcksUnknown);
        assert!(report.target_readback_confirmed&&report.restore_readback_confirmed&&report.original_state_restored);
        assert!(!report.success()&&!report.setter_acks_confirmed&&report.restore_required);
        assert_eq!(report.trial.unwrap().status,ApplyStatus::Unconfirmed);assert_eq!(report.restore.unwrap().status,ApplyStatus::Unconfirmed);
        assert!(c.transport.steps.is_empty());
    }
}
#[test]
fn attempted_trial_com_error_still_reads_and_restores() {
    for setting in Setting::ALL {
        let mut steps=normal_steps(setting);steps[1].response.raw.error=Some(error(ErrorKind::Transport,"error after possible dispatch"));
        let mut c=controller(steps);let report=c.validate(setting);
        assert_eq!(report.trial.as_ref().unwrap().status,ApplyStatus::Failed);
        assert!(report.restore.as_ref().unwrap().write_attempted());assert!(report.original_state_restored);
        assert_eq!(report.outcome,ValidationOutcome::TrialUnconfirmed);assert!(!report.restore_required);
        assert!(c.transport.steps.is_empty());
    }
}
#[test]
fn failed_target_get_does_not_prevent_known_lcd_or_boot_recovery() {
    for setting in [Setting::LcdOverdrive,Setting::BootSound] {
        let mut steps=normal_steps(setting);steps[2]=failed_get(sample(setting,true,75));
        let mut c=controller(steps);let report=c.validate(setting);
        assert!(!report.target_readback_confirmed);assert!(report.restore_readback_confirmed&&report.original_state_restored);
        assert_eq!(report.outcome,ValidationOutcome::TrialUnconfirmed);assert!(c.transport.steps.is_empty());
    }
}
#[test]
fn timeout_restore_uses_fresh_brightness_without_overwriting_concurrent_change() {
    let original=State::BacklightTimeout{enabled:false,brightness:75};let trial=original.with_enabled(true);
    let fresh=State::BacklightTimeout{enabled:true,brightness:25};let restored=fresh.with_enabled(false);
    let mut c=controller(vec![get(original),acked(trial),get(trial),get(fresh),acked(restored),get(restored)]);
    let report=c.validate(Setting::BacklightTimeout);
    assert!(report.restore_readback_confirmed);assert!(!report.original_state_restored);
    assert_eq!(report.outcome,ValidationOutcome::RestoreUnconfirmed);assert!(report.restore_required);
    assert_eq!(report.restore.unwrap().expected,Some(restored));assert!(c.transport.steps.is_empty());
}
#[test]
fn failed_fresh_brightness_get_never_falls_back_to_100_or_saved_byte() {
    for missing_ack in [false,true] {
        let original=State::BacklightTimeout{enabled:false,brightness:75};let trial=original.with_enabled(true);
        let mut fresh=if missing_ack{get(trial)}else{failed_get(trial)};
        if missing_ack{fresh.response.raw.output.remove("ReturnValue");fresh.response.raw.output.insert("unreviewed".into(),RawValue::unsigned(0));}
        let mut c=controller(vec![get(original),acked(trial),failed_get(trial),fresh]);
        let report=c.validate(Setting::BacklightTimeout);
        assert_eq!(report.restore.as_ref().unwrap().status,ApplyStatus::Rejected);
        assert!(!report.restore.unwrap().write_attempted());assert!(report.restore_required);
        assert_eq!(c.transport.calls.iter().filter(|(_,write)|*write).count(),1);
        assert!(c.transport.steps.is_empty());
    }
}
#[test]
fn pre_dispatch_error_requires_no_after_get_or_restore() {
    let original=State::BootSound{enabled:false};let mut write=acked(original.with_enabled(true));
    write.response.attempted=false;write.response.raw.error=Some(error(ErrorKind::PermissionDenied,"input preparation rejected"));
    let mut c=controller(vec![get(original),write]);let report=c.validate(Setting::BootSound);
    assert!(!report.trial.unwrap().write_attempted());assert!(report.restore.is_none());assert!(!report.restore_required);
    assert_eq!(c.transport.calls.len(),2);assert!(c.transport.steps.is_empty());
}
#[test]
fn rejected_restore_ack_is_not_hidden_by_original_readback() {
    let mut steps=normal_steps(Setting::BootSound);steps[3].response.raw.output.insert("gmOutput".into(),RawValue::unsigned(1));
    let mut c=controller(steps);let report=c.validate(Setting::BootSound);
    assert!(report.original_state_restored&&report.restore_readback_confirmed);
    assert_eq!(report.restore.unwrap().status,ApplyStatus::Failed);
    assert_eq!(report.outcome,ValidationOutcome::RestoreUnconfirmed);assert!(report.restore_required);
}
#[test]
fn mismatched_final_get_remains_pending() {
    let mut steps=normal_steps(Setting::LcdOverdrive);steps[4]=get(State::LcdOverdrive{enabled:true});
    let mut c=controller(steps);let report=c.validate(Setting::LcdOverdrive);
    assert!(!report.original_state_restored&&!report.restore_readback_confirmed);assert!(report.restore_required);
    assert_eq!(report.outcome,ValidationOutcome::RestoreUnconfirmed);
}
#[test]
fn exact_target_and_bios_guard_rejects_before_transport_calls() {
    for(model,bios)in [("PHN16-72","V1.18"),("PHN16-71","V1.19"),("","V1.18")] {
        assert!(Controller::new(Fake::new(vec![]),WritePolicy::SupervisedValidation,Identity{model:model.into(),bios:bios.into()}).is_err());
    }
    for model in ["PHN16-71","Predator PHN16-71"] {
        assert!(Controller::new(Fake::new(vec![]),WritePolicy::ReadOnly,Identity{model:model.into(),bios:"V1.18".into()}).is_ok());
    }
}
#[test]
fn raw_u64_inputs_remain_decimal_strings_in_json() {
    let mut c=controller(normal_steps(Setting::BacklightTimeout));let report=c.validate(Setting::BacklightTimeout);
    let json=serde_json::to_value(report).unwrap();
    assert_eq!(json["trial"]["writes"][0]["raw"]["input"]["uiInput"]["value"],set_call(State::BacklightTimeout{enabled:true,brightness:75}).value.to_string());
}

fn fixture(setting:Setting) -> serde_json::Value {
    let source=match setting {
        Setting::LcdOverdrive=>include_str!("../../../../evidence/direct-lcd_overdrive-restore.json"),
        Setting::BootSound=>include_str!("../../../../evidence/direct-boot_sound-restore.json"),
        Setting::BacklightTimeout=>include_str!("../../../../evidence/direct-backlight_timeout-restore.json"),
    };
    serde_json::from_str(source.trim_start_matches('\u{feff}')).unwrap()
}
fn fixture_read(value:&serde_json::Value) -> RawRead { serde_json::from_value(value.clone()).unwrap() }
fn raw_get(setting:Setting,raw:RawRead) -> Step {
    Step{call:get_call(setting),write:false,response:WriteAttempt{attempted:true,raw}}
}
fn raw_set(state:State,raw:RawRead) -> Step {
    Step{call:set_call(state),write:true,response:WriteAttempt{attempted:true,raw}}
}

#[test]
fn real_read_fixture_confirms_only_the_three_exact_getters() {
    let fixture:serde_json::Value=serde_json::from_str(include_str!("../../../../evidence/direct-settings-read.json").trim_start_matches('\u{feff}')).unwrap();
    for reading in fixture["readings"].as_array().unwrap() {
        let setting:Setting=serde_json::from_value(reading["setting"].clone()).unwrap();
        let raw=fixture_read(&reading["raw"]);
        assert!(!raw.output.contains_key("ReturnValue"));assert_eq!(raw.output.len(),1);
        let decoded=read_result(setting,raw.clone());assert_eq!(decoded.ack,Ack::Confirmed);assert!(decoded.state.is_some());
        let mut unreviewed=raw;unreviewed.output.insert("other".into(),RawValue::unsigned(0));
        assert_eq!(read_result(setting,unreviewed).ack,Ack::Unknown);
    }
}
#[test]
fn real_trial_and_restore_fixtures_replay_fixed_setters_and_independent_gets() {
    for setting in Setting::ALL {
        let f=fixture(setting);
        assert_eq!(f["target_readback_confirmed"],true);assert_eq!(f["original_state_restored"],true);
        let original=read_result(setting,fixture_read(&f["original"]["raw"])).state.unwrap();
        let target=original.with_enabled(!original.enabled());
        let mut steps=vec![raw_get(setting,fixture_read(&f["original"]["raw"])),
            raw_set(target,fixture_read(&f["trial"]["writes"][0]["raw"])),
            raw_get(setting,fixture_read(&f["trial"]["after"]["raw"]))];
        let restore_state=if setting==Setting::BacklightTimeout {
            let raw=fixture_read(&f["restore"]["before"]["raw"]);
            let fresh=read_result(setting,raw.clone()).state.unwrap();steps.push(raw_get(setting,raw));
            fresh.with_enabled(original.enabled())
        } else { original };
        steps.extend([raw_set(restore_state,fixture_read(&f["restore"]["writes"][0]["raw"])),
            raw_get(setting,fixture_read(&f["restore"]["after"]["raw"]))]);
        for step in steps.iter().filter(|s|s.write) {
            assert!(!step.response.raw.output.contains_key("ReturnValue"));assert_eq!(step.response.raw.output.len(),1);
            assert_eq!(step.response.raw.output[output_field(step.call)],RawValue::unsigned(0));
        }
        let mut c=controller(steps);let report=c.validate(setting);
        assert!(report.success());assert!(report.setter_acks_confirmed&&report.original_state_restored);
        assert!(!report.restore_required);assert!(c.transport.steps.is_empty());
    }
}
#[test]
fn verified_production_sets_replay_real_ack_and_target_readback() {
    for setting in Setting::ALL {
        let f=fixture(setting);let before=fixture_read(&f["trial"]["before"]["raw"]);
        let original=read_result(setting,before.clone()).state.unwrap();let target=original.with_enabled(!original.enabled());
        let steps=vec![raw_get(setting,before),raw_set(target,fixture_read(&f["trial"]["writes"][0]["raw"])),
            raw_get(setting,fixture_read(&f["trial"]["after"]["raw"]))];
        let mut c=Controller::new(Fake::new(steps),WritePolicy::Verified,identity()).unwrap();
        let report=c.set(setting,target.enabled());assert_eq!(report.status,ApplyStatus::Applied);
        assert_eq!(report.write_ack,Some(Ack::Confirmed));assert!(report.readback_confirmed);assert!(c.transport.steps.is_empty());
    }
}
#[test]
fn verified_never_runs_supervised_toggle_and_restore() {
    for setting in Setting::ALL {
        let original=sample(setting,true,75);
        let mut c=Controller::new(Fake::new(vec![get(original)]),WritePolicy::Verified,identity()).unwrap();
        let report=c.validate(setting);assert_eq!(report.outcome,ValidationOutcome::Rejected);
        assert!(report.trial.is_none());assert!(c.transport.calls.iter().all(|(_,write)|!*write));
    }
}
#[test]
fn missing_return_set_rule_rejects_unreviewed_selectors_bits_and_fields() {
    let cases=[
        Call{class:"AcerGamingFunction",method:"SetGamingProfile",parameter:"gmInput",value:0x11},
        Call{class:"AcerGamingFunction",method:"SetGamingProfile",parameter:"gmInput",value:0x20},
        Call{class:"AcerGamingFunction",method:"SetGamingMiscSetting",parameter:"gmInput",value:7},
        Call{class:"AcerGamingFunction",method:"SetGamingMiscSetting",parameter:"gmInput",value:0x107},
        Call{class:"AcerGamingFunction",method:"SetGamingMiscSetting",parameter:"gmInput",value:0x206},
        Call{class:"APGeAction",method:"SetFunction",parameter:"uiInput",value:4},
        Call{class:"APGeAction",method:"SetFunction",parameter:"uiInput",value:0x88403},
        Call{class:"APGeAction",method:"SetFunction",parameter:"uiInput",value:0x88402|(31<<40)},
        Call{class:"APGeAction",method:"SetFunction",parameter:"uiInput",value:0x88402|(101<<32)},
        Call{class:"APGeAction",method:"SetFunction",parameter:"uiInput",value:0x88402|(1<<48)},
        Call{class:"AcerGamingFunction",method:"SetGamingMiscSetting",parameter:"uiInput",value:6},
    ];
    for call in cases { assert!(!legal_set_call(call));assert_eq!(acknowledge(call,&response(call,0,None),true),Ack::Unknown); }
    for setting in Setting::ALL {
        let call=set_call(sample(setting,true,75));let raw=response(call,0,None);
        assert_eq!(acknowledge(call,&raw,true),Ack::Confirmed);
        let mut extra=raw.clone();extra.input.insert("extra".into(),RawValue::unsigned(0));
        assert_eq!(acknowledge(call,&extra,true),Ack::Rejected);
        let mut extra=raw.clone();extra.output.insert("extra".into(),RawValue::unsigned(0));
        assert_eq!(acknowledge(call,&extra,true),Ack::Unknown);
        let mut other_selector=raw;other_selector.input.insert(call.parameter.into(),RawValue::unsigned(call.value^1));
        assert_eq!(acknowledge(call,&other_selector,true),Ack::Rejected);
    }
}
#[test]
fn reviewed_setter_schema_never_hides_explicit_errors_or_wrong_output_type() {
    for setting in Setting::ALL {
        let call=set_call(sample(setting,true,75));
        for ret in [RawValue::Bool(false),RawValue::unsigned(0),RawValue::Text("true".into())] {
            assert_eq!(acknowledge(call,&response(call,0,Some(ret)),true),Ack::Rejected);
        }
        assert_eq!(acknowledge(call,&response(call,1,None),true),Ack::Rejected);
        for value in [RawValue::Signed("0".into()),RawValue::Text("0".into()),RawValue::Bool(false),RawValue::Null] {
            let raw=call.raw(BTreeMap::from([(output_field(call).into(),value)]),None);
            assert_eq!(acknowledge(call,&raw,true),Ack::Unknown);
        }
        let mut raw=response(call,0,None);raw.error=Some(error(ErrorKind::Transport,"simulated firmware dispatch error"));
        assert_eq!(acknowledge(call,&raw,true),Ack::Rejected);
    }
}
#[test]
fn unknown_brightness_above_official_hundred_never_dispatches_a_write() {
    for brightness in [101,150,255] {
        let state=State::BacklightTimeout{enabled:false,brightness};
        assert!(decode(Setting::BacklightTimeout,get_word(state)).is_none());assert!(!legal_set_call(set_call(state)));
        for policy in [WritePolicy::Verified,WritePolicy::SupervisedValidation] {
            let mut c=Controller::new(Fake::new(vec![get(state)]),policy,identity()).unwrap();
            let report=c.set(Setting::BacklightTimeout,true);assert_eq!(report.status,ApplyStatus::Rejected);
            assert!(!report.write_attempted());assert_eq!(c.transport.calls.len(),1);
        }
        let mut c=controller(vec![]);let report=c.write_known(state,None);
        assert_eq!(report.status,ApplyStatus::Rejected);assert!(c.transport.calls.is_empty());
    }
}
