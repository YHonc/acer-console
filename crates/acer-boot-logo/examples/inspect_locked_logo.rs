fn main(){
    if std::env::args_os().len()!=1{eprintln!("inspect_locked_logo takes no arguments and never writes");std::process::exit(2)}
    match acer_boot_logo::native_transaction::inspect_current(){
        Ok(value)=>println!("{}",serde_json::to_string_pretty(&value).unwrap()),
        Err(e)=>{eprintln!("{e}");std::process::exit(1)}
    }
}
