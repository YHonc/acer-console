use acer_broker::audio_controls::{AudioClient,AudioPreset,WritePolicy};
fn main() {
    let args:Vec<_>=std::env::args().skip(1).collect();
    let result:Result<(serde_json::Value,bool),acer_protocol::DeviceError>=(||{
        match args.as_slice() {
            [] => {let report=AudioClient::connect()?.query()?;let ok=report.state.value.is_some()&&report.capability.value.is_some();Ok((serde_json::to_value(report).unwrap(),ok))},
            [flag,mode] if flag=="--validate-restore" => {
                let preset:AudioPreset=serde_json::from_value(serde_json::Value::String(mode.clone())).map_err(|e|acer_protocol::DeviceError{kind:acer_protocol::ErrorKind::InvalidArgument,message:e.to_string()})?;
                let report=AudioClient::connect_with_policy(WritePolicy::SupervisedValidation)?.validate_restore(preset)?;
                let ok=report.success();Ok((serde_json::to_value(report).unwrap(),ok))
            },
            _=>{eprintln!("Usage: audio_probe [--validate-restore music|movie|voice|strategy|rpg|shooter|custom|automatic]");std::process::exit(2);}
        }
    })();
    match result { Ok((value,ok))=>{println!("{}",serde_json::to_string_pretty(&value).unwrap());if !ok{std::process::exit(1)}},Err(e)=>{println!("{}",serde_json::to_string_pretty(&e).unwrap());std::process::exit(1)} }
}
