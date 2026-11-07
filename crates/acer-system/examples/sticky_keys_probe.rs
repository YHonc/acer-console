fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let output = match args.as_slice() {
        [] => serde_json::to_value(acer_system::sticky_keys::get()),
        [arg] if arg == "--validate-restore" => serde_json::to_value(acer_system::sticky_keys::validate_restore()),
        _ => { eprintln!("Usage: sticky_keys_probe [--validate-restore]"); std::process::exit(2); }
    }.expect("serialize probe report");
    println!("{}", serde_json::to_string_pretty(&output).unwrap());
    if output.get("Err").is_some() || output.pointer("/Ok/restored").is_some_and(|v| v != true)
        || output.pointer("/Ok/trial/Err").is_some() { std::process::exit(1); }
}
