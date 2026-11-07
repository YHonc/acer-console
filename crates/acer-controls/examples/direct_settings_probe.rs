use acer_controls::direct_settings::{DirectSettingsDevice,Setting,WritePolicy};

#[derive(Debug,Clone,Copy,PartialEq,Eq)]
enum Mode { Read, Validate(Setting) }
const USAGE:&str="Usage: direct_settings_probe --read | --validate-restore lcd_overdrive|boot_sound|backlight_timeout\nRead is default-policy only. Validation changes one fixed setting then attempts original recovery; unknown GET ACK/state never writes. The production helper uses Verified policy; this probe exposes only read and supervised restore validation.";
fn parse(args:&[String]) -> Option<Mode> { match args {
    [flag] if flag=="--read" => Some(Mode::Read),
    [flag,setting] if flag=="--validate-restore" => match setting.as_str() {
        "lcd_overdrive"=>Some(Mode::Validate(Setting::LcdOverdrive)),
        "boot_sound"=>Some(Mode::Validate(Setting::BootSound)),
        "backlight_timeout"=>Some(Mode::Validate(Setting::BacklightTimeout)), _=>None,
    }, _=>None,
} }
fn main() {
    let args:Vec<String>=std::env::args().skip(1).collect();
    let Some(mode)=parse(&args) else { eprintln!("{USAGE}");std::process::exit(2); };
    let result=(|| -> Result<(serde_json::Value,bool),acer_protocol::DeviceError> {
        let policy=if matches!(mode,Mode::Read) { WritePolicy::ReadOnly } else { WritePolicy::SupervisedValidation };
        let device=DirectSettingsDevice::connect_with_policy(policy)?;
        let json_error=|e:serde_json::Error|acer_protocol::DeviceError { kind:acer_protocol::ErrorKind::Protocol,message:e.to_string() };
        match mode {
            Mode::Read=>{let report=device.read_all()?;let success=report.readings.iter().all(|r|r.state.is_some());Ok((serde_json::to_value(report).map_err(json_error)?,success))},
            Mode::Validate(setting)=>{let report=device.validate_restore(setting)?;let success=report.success();Ok((serde_json::to_value(report).map_err(json_error)?,success))},
        }
    })();
    match result { Ok((report,success))=>{println!("{}",serde_json::to_string_pretty(&report).unwrap());if !success { std::process::exit(1); }},
        Err(error)=>{println!("{}",serde_json::json!({"error":error}));std::process::exit(1);},
    }
}
#[cfg(test)] mod tests {
    use super::*;
    #[test] fn only_fixed_explicit_modes_are_accepted() {
        assert_eq!(parse(&["--read".into()]),Some(Mode::Read));
        for(name,setting)in [("lcd_overdrive",Setting::LcdOverdrive),("boot_sound",Setting::BootSound),("backlight_timeout",Setting::BacklightTimeout)] {
            assert_eq!(parse(&["--validate-restore".into(),name.into()]),Some(Mode::Validate(setting)));
        }
        for args in [vec![],vec!["--validate-restore"],vec!["--validate-restore","brightness"],vec!["--read","--validate-restore","boot_sound"],vec!["--set","APGeAction","SetFunction","0"]] {
            assert_eq!(parse(&args.into_iter().map(str::to_string).collect::<Vec<_>>()),None);
        }
    }
}
