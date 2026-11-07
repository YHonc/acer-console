fn main(){
    if std::env::args_os().len()!=1{eprintln!("calibration_power_read is read-only and accepts no arguments");std::process::exit(2)}
    match acer_system::calibration_power::capture(){
        Ok(snapshot)=>println!("{}",serde_json::to_string_pretty(&serde_json::json!({"order":acer_system::calibration_power::SETTINGS,"snapshot":snapshot,"writes_attempted":false})).unwrap()),
        Err(e)=>{eprintln!("{e}");std::process::exit(1)}
    }
}
