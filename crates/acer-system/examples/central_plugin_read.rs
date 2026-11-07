fn main() {
    match acer_system::central_plugin::query() {
        Ok(state)=>println!("{}",serde_json::to_string_pretty(&state).unwrap()),
        Err(error)=>{eprintln!("{error}");std::process::exit(1);}
    }
}
