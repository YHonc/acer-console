//! Read-only native discovery. Never mounts volumes or invokes vendor logo operations.
fn main(){
    if std::env::args().len()!=1{eprintln!("inspect_logo accepts no arguments and is read only");std::process::exit(2)}
    match acer_boot_logo::native_discovery::discover(){
        Ok(discovery)=>{
            let state=acer_boot_logo::Targets::inspect(&discovery.esp_oem,&discovery.cache).and_then(|targets|targets.states());
            println!("{}",serde_json::json!({"discovery":discovery,"states":state.as_ref().ok(),"state_error":state.err().map(|e|e.to_string()),"read_only":true}));
        },
        Err(error)=>{println!("{}",serde_json::json!({"error":error,"read_only":true}));std::process::exit(1)},
    }
}
