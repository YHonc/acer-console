//! Native thread smoke check. Does not send Start, Recover, Stop or Abort.
use std::time::{Duration,Instant};
fn main()->Result<(),String>{
    #[cfg(windows)]{
        let stamp=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|e|e.to_string())?.as_nanos();
        let root=std::env::temp_dir().join(format!("acer-calibration-idle-{stamp}"));
        let mut worker=acer_broker::calibration_worker::Worker::spawn(root.clone())?;
        println!("status={}",worker.status()?);let started=Instant::now();worker.shutdown(Duration::from_secs(3))?;
        println!("worker_exited=true; shutdown_ms={}; directory_created={}; calibration_started=false",started.elapsed().as_millis(),root.exists());
        if root.exists(){return Err("Idle worker unexpectedly created a data directory".into())}
    }
    Ok(())
}
