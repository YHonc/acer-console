use acer_protocol::*;

/// Called only after an authorized custom-fan transaction has attempted writes.
/// Recovery is one fixed Auto/Max command, never another custom-speed transaction.
pub(crate) fn recover_custom_failure(
    mut report: ApplyReport,
    restore: impl FnOnce(FanMode) -> (RawRead, Option<Snapshot>),
) -> ApplyReport {
    if matches!(report.status, ApplyStatus::Applied | ApplyStatus::Rejected) || report.writes.is_empty() { return report; }
    let mode = match report.before.as_ref().and_then(|s| s.fan_mode) {
        Some(FanMode::Max) => FanMode::Max,
        _ => FanMode::Auto,
    };
    let (write, after) = restore(mode);
    let confirmed = verified_fan_result(mode, std::slice::from_ref(&write), after.as_ref()) == ApplyStatus::Applied;
    report.writes.push(write);
    report.after = after;
    // Successful rollback never turns the original failed request into Applied.
    report.message.push_str(&format!("; custom-fan recovery to {mode:?}: {}", if confirmed { "ACK and mode readback confirmed" } else { "NOT confirmed; automatic control must remain stopped" }));
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    fn report(status: ApplyStatus) -> ApplyReport {
        let call=encode(&Command::SetFanSpeeds{cpu_percent:70,gpu_percent:70}).unwrap().remove(0);
        ApplyReport {status,message:"original failure".into(),before:None,after:None,writes:vec![RawRead{class:call.class,method:call.method,input:call.input,output:Default::default(),error:None}]}
    }
    #[test] fn failed_and_unknown_transactions_attempt_auto_once() {
        for status in [ApplyStatus::Failed,ApplyStatus::Unconfirmed] {
            let mut count=0;
            let result=recover_custom_failure(report(status),|mode| {
                count+=1; assert_eq!(mode,FanMode::Auto);
                let call=encode(&Command::SetFanMode{mode}).unwrap().remove(0);
                (RawRead{class:call.class,method:call.method,input:call.input,output:Default::default(),error:None},None)
            });
            assert_eq!(count,1); assert_eq!(result.status,status); assert_eq!(result.writes.len(),2);
            assert!(result.message.contains("NOT confirmed"));
        }
    }
    #[test] fn successful_and_rejected_requests_never_recover() {
        for status in [ApplyStatus::Applied,ApplyStatus::Rejected] {
            let result=recover_custom_failure(report(status),|_|panic!("unexpected write"));
            assert_eq!(result.status,status);
        }
    }
    #[test] fn prior_max_is_preserved_but_prior_custom_returns_to_auto() {
        for (prior,expected) in [(FanMode::Max,FanMode::Max),(FanMode::Custom,FanMode::Auto),(FanMode::Auto,FanMode::Auto)] {
            let mut input=report(ApplyStatus::Failed);
            input.before=Some(Snapshot { captured_at_ms:0,cpu_temperature_c:None,gpu_temperature_c:None,cpu_fan_rpm:None,gpu_fan_rpm:None,cpu_fan_percent:None,gpu_fan_percent:None,profile:None,fan_mode:Some(prior),lighting:None,lighting_effect:None,lighting_config:None,battery_care:None,ac_online:None,battery_percent:None,reads:vec![] });
            recover_custom_failure(input,|mode| {
                assert_eq!(mode,expected);
                let call=encode(&Command::SetFanMode{mode}).unwrap().remove(0);
                (RawRead{class:call.class,method:call.method,input:call.input,output:Default::default(),error:None},None)
            });
        }
    }
}
