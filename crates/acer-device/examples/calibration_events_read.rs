//! Read-only subscription/exit smoke check. Never writes a hardware command.
use acer_device::calibration_events::Listener;
use std::time::{Duration,Instant};
fn main()->Result<(),String>{
    let mut listener=Listener::start(Duration::from_secs(10))?;
    println!("subscription_ready=true; calibration_started=false");
    let deadline=Instant::now()+Duration::from_secs(2);let mut observed=0;
    while Instant::now()<deadline{
        while let Ok(event)=listener.try_recv(){observed+=1;println!("event={:?}; bytes={:?}",event.event,event.detail)}
        std::thread::sleep(Duration::from_millis(50));
    }
    let started=Instant::now();listener.stop(Duration::from_secs(5))?;
    println!("listener_exited=true; stop_elapsed_ms={}; events_observed={observed}",started.elapsed().as_millis());Ok(())
}
