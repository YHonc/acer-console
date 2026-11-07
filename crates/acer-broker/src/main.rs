#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
fn main() {
    if let Err(error) = acer_broker::run_helper() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
