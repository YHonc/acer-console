fn main(){
    // Isolated, unbound read: never loads user credentials, launches an app or writes startup.
    let dir=std::env::temp_dir().join(format!("acer-dc-read-only-{}",std::process::id()));
    match acer_system::device_center_plugin::query(&dir){
        Ok(value)=>println!("{}",serde_json::to_string_pretty(&value).unwrap()),
        Err(e)=>{eprintln!("{e}");std::process::exit(1);}
    }
}
