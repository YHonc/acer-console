//! Explicit native elevation smoke test. Never calls apply or writes hardware.
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args != ["--authorize"] {
        eprintln!("Usage: read_smoke.exe --authorize\nPlace acer-helper.exe beside this executable first. This explicitly requests UAC and performs read-only RPCs.");
        std::process::exit(2);
    }
    if let Err(err) = run() {
        eprintln!("Broker smoke test failed: {err}");
        std::process::exit(1);
    }
}
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let client = acer_broker::BrokerClient::launch_user_initiated()?;
    let first = client.capabilities()?;
    println!("{}", serde_json::to_string(&first)?);
    let snapshot = client.snapshot()?;
    println!("{}", serde_json::to_string(&snapshot)?);
    let last = client.capabilities()?;
    println!("{}", serde_json::to_string(&last)?);
    // Reconnection and distinct request IDs were exercised by the three calls.
    // Returned fields and raw reads still require hardware-specific verification.
    Ok(())
}
