fn main() {
    let args:Vec<String>=std::env::args().skip(1).collect();
    let (validate,threshold)=match args.as_slice(){
        []=>(false,None),
        [arg]if arg=="--validate-switch-restore"=>(true,None),
        [arg,value]if arg=="--validate-threshold-restore"=>match value.parse::<u8>().ok().and_then(|v|acer_controls::UsbThreshold::try_from(v).ok()){
            Some(threshold)=>(true,Some(threshold)),None=>{eprintln!("USB threshold must be 10, 20 or 30");std::process::exit(2)}
        },
        _=>{eprintln!("Usage: usb_probe [--validate-switch-restore | --validate-threshold-restore 10|20|30]");std::process::exit(2)}
    };
    let policy=if validate{acer_controls::WritePolicy::SupervisedValidation}else{acer_controls::WritePolicy::ReadOnly};
    let result=(||->Result<serde_json::Value,acer_protocol::DeviceError>{
        let device=acer_controls::UsbDevice::connect_with_policy(policy)?;
        if validate{let report=match threshold {Some(value)=>device.validate_threshold_restore(value)?,None=>device.validate_switch_restore()?};serde_json::to_value(report).map_err(|e|acer_protocol::DeviceError{kind:acer_protocol::ErrorKind::Protocol,message:e.to_string()})}
        else{serde_json::to_value(device.get()?).map_err(|e|acer_protocol::DeviceError{kind:acer_protocol::ErrorKind::Protocol,message:e.to_string()})}
    })();
    match result{
        Ok(value)=>{
            let failed=if validate{value["trial"]["status"]!="applied"||value["restored"]!=true}else{value["state"].is_null()};
            println!("{}",serde_json::to_string_pretty(&value).unwrap());if failed{std::process::exit(1)}
        },
        Err(error)=>{println!("{}",serde_json::json!({"error":error,"validation_requested":validate}));std::process::exit(1)},
    }
}
