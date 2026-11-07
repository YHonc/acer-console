//! Read-only registration/query/unregistration check; never requests suspend.
fn main() -> Result<(),Box<dyn std::error::Error>> {
    use acer_system::power_watch::{get_power_status,PowerWatcher};
    use std::time::{Duration,Instant};
    let mut watcher=PowerWatcher::start(8)?;
    println!("registered={} status={}",watcher.is_running(),serde_json::to_string(&get_power_status()?)?);
    let until=Instant::now()+Duration::from_secs(3);
    while Instant::now()<until {
        match watcher.recv_timeout(Duration::from_millis(200)) {
            Ok(event)=>println!("event={event:?} status={}",serde_json::to_string(&get_power_status()?)?),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout)=>{},
            Err(err)=>return Err(err.into()),
        }
    }
    println!("coalesced={}",watcher.dropped_events());
    watcher.shutdown()?;
    println!("stopped={}",!watcher.is_running());
    Ok(())
}
