//! Explicit end-to-end test of the fixed official startup RPC. No hardware writes.
use acer_broker::{BrokerClient, OfficialAction};
use serde_json::{json, Value};

fn main() {
    if std::env::args().skip(1).collect::<Vec<_>>() != ["--authorize-roundtrip"] {
        eprintln!("Usage: official_task_smoke.exe --authorize-roundtrip\nRequires adjacent acer-helper.exe. Backs up, disables and restores the fixed official task.");
        std::process::exit(2);
    }
    match run() {
        Ok(report) => { let success=report["success"]==true; println!("{report}"); if !success {std::process::exit(1)} },
        Err(error) => {println!("{}",json!({"success":false,"error":error})); std::process::exit(1)},
    }
}

fn call(client:&BrokerClient,action:OfficialAction)->Result<Value,String>{
    client.official_task(action).map_err(|e|e.to_string())
}
fn good(value:&Value)->bool{value.get("error").is_some_and(Value::is_null)}
fn run()->Result<Value,String>{
    let client=BrokerClient::launch_user_initiated().map_err(|e|e.to_string())?;
    let before=call(&client,OfficialAction::Query)?;
    if !good(&before)||before["enabled"].as_bool().is_none(){return Err(format!("Cannot establish initial state: {before}"))}
    let backup=call(&client,OfficialAction::Backup)?;
    if !good(&backup)||backup["backup_valid"]!=true||backup["backup_original_enabled"]!=before["enabled"]{
        return Err(format!("Backup is not a matching recovery point: {backup}"));
    }
    let disabled=call(&client,OfficialAction::Disable);
    // Always attempt restoration once the disable request has been sent, including transport errors.
    let restored=call(&client,OfficialAction::Restore);
    let after=call(&client,OfficialAction::Query);
    let success=disabled.as_ref().is_ok_and(|v|good(v)&&v["enabled"]==false)
        && restored.as_ref().is_ok_and(|v|good(v)&&v["enabled"]==before["enabled"])
        && after.as_ref().is_ok_and(|v|good(v)&&v["enabled"]==before["enabled"]);
    Ok(json!({"success":success,"before":before,"backup":backup,"disabled":disabled,"restored":restored,"after":after}))
}
