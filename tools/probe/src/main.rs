//! Default commands are read only; hardware validation requires one exact flag.
mod validation;
mod profile_validation;
mod custom_validation;
mod lighting_validation;
mod color_validation;
mod battery_validation;
mod dynamic_validation;
mod verified_lighting_validation;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode { Snapshot, Capabilities, ValidateFan, ValidateProfile, ValidateCustom, ValidateLighting, ValidateColors, ValidateBattery, ValidateProfileTarget(acer_protocol::Profile),ValidateEffect(acer_protocol::LightingEffect), ValidateVerifiedLighting }
const USAGE:&str="Usage: acer-probe [--help | --capabilities | --validate-fan-max-restore | --validate-profile-balanced-restore | --validate-fan-custom-restore | --validate-profile quiet|turbo --restore | --validate-lighting-brightness-restore | --validate-lighting-static-colors-restore | --validate-battery-care-restore | --validate-lighting-effect 1..7 --restore | --validate-lighting-verified-restore]\nDefault and --capabilities are read only. Explicit validation commands change hardware and restore saved settings.";
fn parse(args: &[String]) -> Option<Mode> {
    match args {
        [] => Some(Mode::Snapshot),
        [flag] if flag == "--capabilities" => Some(Mode::Capabilities),
        [flag] if flag == "--validate-fan-max-restore" => Some(Mode::ValidateFan),
        [flag] if flag == "--validate-profile-balanced-restore" => Some(Mode::ValidateProfile),
        [flag] if flag == "--validate-fan-custom-restore" => Some(Mode::ValidateCustom),
        [flag] if flag == "--validate-lighting-brightness-restore"=>Some(Mode::ValidateLighting),
        [flag] if flag == "--validate-lighting-static-colors-restore"=>Some(Mode::ValidateColors),
        [flag] if flag=="--validate-battery-care-restore"=>Some(Mode::ValidateBattery),
        [flag] if flag=="--validate-lighting-verified-restore"=>Some(Mode::ValidateVerifiedLighting),
        [flag,target,restore]if flag=="--validate-profile"&&restore=="--restore"=>match target.as_str(){"quiet"=>Some(Mode::ValidateProfileTarget(acer_protocol::Profile::Quiet)),"turbo"=>Some(Mode::ValidateProfileTarget(acer_protocol::Profile::Turbo)),_=>None},
        [flag,effect,restore]if flag=="--validate-lighting-effect"&&restore=="--restore"=>effect.parse::<u8>().ok().and_then(acer_protocol::LightingEffect::from_code).filter(|e|*e!=acer_protocol::LightingEffect::Static).map(Mode::ValidateEffect),
        _ => None,
    }
}
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len()==1&&args[0]=="--help"{println!("{USAGE}");return;}
    let Some(mode) = parse(&args) else {
        eprintln!("{USAGE}");
        std::process::exit(2);
    };
    match run(mode) {
        Ok((value, success)) => { println!("{}", serde_json::to_string_pretty(&value).unwrap()); if !success { std::process::exit(1); } }
        Err(error) => { eprintln!("{}", serde_json::to_string_pretty(&error).unwrap()); std::process::exit(1); }
    }
}
fn run(mode: Mode) -> Result<(serde_json::Value, bool), acer_device::DeviceError> {
    if mode==Mode::ValidateVerifiedLighting {
        let checkpoint=acer_device::Device::connect_with_policy(acer_device::WritePolicy::AllowUnverified)?;
        let verified=acer_device::Device::connect_with_policy(acer_device::WritePolicy::Verified)?;
        let report=verified_lighting_validation::run(&checkpoint,&verified);let success=report.success();
        return serde_json::to_value(report).map(|value|(value,success)).map_err(json_error);
    }
    let device = match mode {
        Mode::ValidateFan|Mode::ValidateProfile|Mode::ValidateCustom|Mode::ValidateLighting|Mode::ValidateColors|Mode::ValidateBattery|Mode::ValidateProfileTarget(_)|Mode::ValidateEffect(_) => acer_device::Device::connect_with_policy(acer_device::WritePolicy::AllowUnverified)?,
        _ => acer_device::Device::connect()?,
    };
    let value = match mode {
        Mode::Snapshot => serde_json::to_value(device.snapshot()?),
        Mode::Capabilities => serde_json::to_value(device.capabilities()?),
        Mode::ValidateFan => { let report = validation::run(&device); let success = report.success(); return serde_json::to_value(report).map(|value| (value, success)).map_err(json_error); }
        Mode::ValidateProfile => {let report=profile_validation::run(&device);let success=report.success();return serde_json::to_value(report).map(|value|(value,success)).map_err(json_error);}
        Mode::ValidateCustom => {let report=custom_validation::run(&device);let success=report.success();return serde_json::to_value(report).map(|value|(value,success)).map_err(json_error);}
        Mode::ValidateLighting=>{let report=lighting_validation::run(&device);let success=report.success();return serde_json::to_value(report).map(|value|(value,success)).map_err(json_error);}
        Mode::ValidateColors=>{let report=color_validation::run(&device);let success=report.success();return serde_json::to_value(report).map(|value|(value,success)).map_err(json_error);}
        Mode::ValidateBattery=>{let report=battery_validation::run(&device);let success=report.success();return serde_json::to_value(report).map(|value|(value,success)).map_err(json_error);}
        Mode::ValidateProfileTarget(target)=>{let report=profile_validation::run_target(&device,target);let success=report.success();return serde_json::to_value(report).map(|value|(value,success)).map_err(json_error);}
        Mode::ValidateEffect(effect)=>{let report=dynamic_validation::run(&device,effect);let success=report.success();return serde_json::to_value(report).map(|value|(value,success)).map_err(json_error);}
        Mode::ValidateVerifiedLighting=>unreachable!("separate native checkpoint and Verified sessions above"),
    };
    value.map(|value| (value, true)).map_err(json_error)
}
fn json_error(error: serde_json::Error) -> acer_device::DeviceError { acer_device::DeviceError { kind: acer_protocol::ErrorKind::Protocol, message: error.to_string() } }
#[cfg(test)] mod tests {
    use super::*;
    #[test] fn hardware_validation_needs_one_exact_flag() {
        assert_eq!(parse(&[]), Some(Mode::Snapshot));
        assert_eq!(parse(&["--capabilities".into()]), Some(Mode::Capabilities));
        assert_eq!(parse(&["--validate-fan-max-restore".into()]), Some(Mode::ValidateFan));
        assert_eq!(parse(&["--validate-profile-balanced-restore".into()]),Some(Mode::ValidateProfile));
        assert_eq!(parse(&["--validate-fan-custom-restore".into()]),Some(Mode::ValidateCustom));
        assert_eq!(parse(&["--validate-lighting-brightness-restore".into()]),Some(Mode::ValidateLighting));
        assert_eq!(parse(&["--validate-lighting-static-colors-restore".into()]),Some(Mode::ValidateColors));
        assert_eq!(parse(&["--validate-battery-care-restore".into()]),Some(Mode::ValidateBattery));
        assert_eq!(parse(&["--validate-lighting-verified-restore".into()]),Some(Mode::ValidateVerifiedLighting));
        assert_eq!(parse(&["--validate-lighting-verified-restore".into(),"--capabilities".into()]),None);
        assert_eq!(parse(&["--validate-profile".into(),"quiet".into(),"--restore".into()]),Some(Mode::ValidateProfileTarget(acer_protocol::Profile::Quiet)));
        assert_eq!(parse(&["--validate-profile".into(),"turbo".into(),"--restore".into()]),Some(Mode::ValidateProfileTarget(acer_protocol::Profile::Turbo)));
        assert_eq!(parse(&["--validate-profile".into(),"eco".into(),"--restore".into()]),None);
        assert_eq!(parse(&["--validate-profile".into(),"quiet".into()]),None);
        assert_eq!(parse(&["--validate-lighting-effect".into(),"1".into(),"--restore".into()]),Some(Mode::ValidateEffect(acer_protocol::LightingEffect::Breathing)));
        assert_eq!(parse(&["--validate-lighting-effect".into(),"0".into(),"--restore".into()]),None);
        assert_eq!(parse(&["--validate-lighting-effect".into(),"8".into(),"--restore".into()]),None);
        assert_eq!(parse(&["--validate-lighting-effect".into(),"1".into()]),None);
        assert_eq!(parse(&["--validate-fan-max-restore".into(), "--capabilities".into()]), None);
        assert_eq!(parse(&["--write".into()]), None);
    }
}
