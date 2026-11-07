fn main(){
    if let Err(error)=run(){eprintln!("{error}");std::process::exit(1)}
}
fn run()->Result<(),Box<dyn std::error::Error>>{
    let args:Vec<_>=std::env::args().skip(1).collect();
    let initial=acer_system::get_official_task()?;
    if args.is_empty(){println!("{}",serde_json::to_string_pretty(&initial)?);return Ok(())}
    if args.len()!=2||args[0]!="--validate"{return Err("Use no args for read-only query, or --validate <new-backup-directory>".into())}
    let path=std::path::PathBuf::from(&args[1]);
    acer_system::backup_official_task(&path)?;
    // No early return between the first mutation attempt and restoration.
    let disabled=acer_system::disable_official_task(&path).map_err(|e|e.to_string());
    let restored=acer_system::restore_official_task(&path).map_err(|e|e.to_string());
    let confirmed=restored.as_ref().is_ok_and(|s|s.enabled==initial.enabled&&s.definition_sha256==initial.definition_sha256);
    let success=disabled.as_ref().is_ok_and(|s|!s.enabled)&&confirmed;
    println!("{}",serde_json::to_string_pretty(&serde_json::json!({"initial":initial,"disabled":disabled,"restored":restored,"restore_confirmed":confirmed,"success":success}))?);
    if !success{return Err("Task roundtrip did not fully confirm; inspect saved report and backup".into())}Ok(())
}
