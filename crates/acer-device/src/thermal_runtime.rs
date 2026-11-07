//! Serial curve runtime; the owning COM thread supplies all I/O.
use acer_protocol::*;
use acer_rules::thermal::{Curve, Decision, Engine, Sample};
use serde::{Deserialize, Serialize};
use std::time::Instant;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThermalConfig { pub cpu: Curve, pub gpu: Curve }
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ThermalStatus {
    pub active: bool,
    pub restore_pending: bool,
    pub message: String,
    pub cpu_target: Option<u8>,
    pub gpu_target: Option<u8>,
    pub last_report: Option<ApplyReport>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use acer_rules::thermal::Point;
    struct Fake { snapshot: Snapshot, writes: usize, restores: usize, fail_write: bool, fail_restore: bool }
    fn fake() -> Fake {
        let reads=[(1,0x100),(8,0x4000),(9,0x4100)].into_iter().map(|(selector,value)| {
            let call=gaming("GetGamingFanBehavior",selector);
            RawRead {class:call.class,method:call.method,input:call.input,output:std::collections::BTreeMap::from([("gmOutput".into(),RawValue::unsigned(value))]),error:None}
        }).collect();
        Fake {snapshot:Snapshot {captured_at_ms:0,cpu_temperature_c:Some(60.0),gpu_temperature_c:Some(60.0),cpu_fan_rpm:Some(3000.0),gpu_fan_rpm:Some(3000.0),cpu_fan_percent:Some(70),gpu_fan_percent:Some(70),profile:None,fan_mode:Some(FanMode::Auto),lighting:None,lighting_effect:None,lighting_config:None,battery_care:None,ac_online:Some(true),battery_percent:Some(100),reads},writes:0,restores:0,fail_write:false,fail_restore:false}
    }
    fn config() -> ThermalConfig {
        let c=Curve {points:vec![Point{celsius:40.0,percent:40},Point{celsius:80.0,percent:100}],hysteresis_celsius:3.0,minimum_write_interval_ms:1000,maximum_sample_age_ms:5000};
        ThermalConfig{cpu:c.clone(),gpu:c}
    }
    impl Fake {
        fn set_mode(&mut self,mode:FanMode){
            self.snapshot.fan_mode=Some(mode);
            let code=match mode{FanMode::Auto=>1u64,FanMode::Max=>2,FanMode::Custom=>3};
            for read in &mut self.snapshot.reads {
                let value=match read.input.get("gmInput").and_then(RawValue::as_u64){Some(1)=>code<<8,Some(8)=>code<<14,Some(9)=>(code<<8)|(code<<14),_=>continue};
                read.output.insert("gmOutput".into(),RawValue::unsigned(value));
            }
        }
        fn report(&self,failed:bool)->ApplyReport {ApplyReport{status:if failed{ApplyStatus::Failed}else{ApplyStatus::Applied},message:"fake".into(),before:None,after:Some(self.snapshot.clone()),writes:vec![]}}
    }
    impl ThermalIo for Fake {
        fn ready(&mut self)->Result<bool,DeviceError>{Ok(true)}
        fn read(&mut self)->Result<Snapshot,DeviceError>{Ok(self.snapshot.clone())}
        fn write(&mut self,cpu:u8,gpu:u8)->Result<ApplyReport,DeviceError>{self.writes+=1;self.set_mode(FanMode::Custom);self.snapshot.cpu_fan_percent=Some(cpu);self.snapshot.gpu_fan_percent=Some(gpu);Ok(self.report(self.fail_write))}
        fn restore_auto(&mut self)->Result<ApplyReport,DeviceError>{self.restores+=1;if !self.fail_restore{self.set_mode(FanMode::Auto);}Ok(self.report(self.fail_restore))}
    }
    fn tick(session:&mut Session,io:&mut Fake,now:u64){let snapshot=io.read();session.tick_sample(now,now,snapshot,io);}
    #[test] fn arm_is_read_only_and_stop_restores_once() {
        let mut io=fake();let mut s=Session::start(config(),&mut io).unwrap();assert_eq!(io.writes,0);
        tick(&mut s,&mut io,0);assert_eq!(io.writes,1);assert_eq!(s.status.cpu_target,Some(70));
        tick(&mut s,&mut io,1000);assert_eq!(io.writes,1);
        s.stop(&mut io,"test");s.stop(&mut io,"again");assert_eq!(io.restores,1);assert!(!s.owns_fans());
    }
    #[test] fn power_loss_and_external_override_stop_before_next_write() {
        for power_loss in [true,false] {
            let mut io=fake();let mut s=Session::start(config(),&mut io).unwrap();tick(&mut s,&mut io,0);
            if power_loss{io.snapshot.ac_online=Some(false);}else{io.snapshot.cpu_fan_percent=Some(80);}
            tick(&mut s,&mut io,1000);assert_eq!(io.writes,1);assert_eq!(io.restores,1);assert!(!s.status.active);
        }
    }
    #[test] fn failed_restoration_blocks_ownership_until_explicit_retry() {
        let mut io=fake();let mut s=Session::start(config(),&mut io).unwrap();io.fail_write=true;io.fail_restore=true;
        tick(&mut s,&mut io,0);assert!(!s.status.active);assert!(s.owns_fans());assert!(s.status.restore_pending);
        tick(&mut s,&mut io,1000);assert_eq!(io.restores,1);
        io.fail_restore=false;s.stop(&mut io,"retry");assert!(!s.owns_fans());
    }
    #[test] fn sampling_gap_and_slow_read_restore() {
        let mut io=fake();let mut s=Session::start(config(),&mut io).unwrap();tick(&mut s,&mut io,0);tick(&mut s,&mut io,5001);
        assert_eq!(io.writes,1);assert!(!s.status.active);
        let mut io=fake();let mut s=Session::start(config(),&mut io).unwrap();let snapshot=io.read();s.tick_sample(0,5001,snapshot,&mut io);
        assert_eq!(io.writes,0);assert_eq!(io.restores,1);
    }
    #[test] fn missing_sensor_never_becomes_zero_target() {
        let mut io=fake();let mut s=Session::start(config(),&mut io).unwrap();io.snapshot.gpu_temperature_c=None;tick(&mut s,&mut io,0);
        assert_eq!(io.writes,0);assert_eq!(io.restores,1);
    }
    #[test] fn pair_obeys_slower_interval_but_sensor_failure_is_immediate() {
        let mut io=fake();let mut config=config();config.gpu.minimum_write_interval_ms=3000;
        let mut s=Session::start(config.clone(),&mut io).unwrap();tick(&mut s,&mut io,0);
        io.snapshot.cpu_temperature_c=Some(75.0);tick(&mut s,&mut io,1000);tick(&mut s,&mut io,2000);assert_eq!(io.writes,1);
        tick(&mut s,&mut io,3000);assert_eq!(io.writes,2);
        let mut io=fake();let mut s=Session::start(config,&mut io).unwrap();tick(&mut s,&mut io,0);
        io.snapshot.gpu_temperature_c=None;tick(&mut s,&mut io,1000);assert_eq!(io.restores,1);assert!(!s.active());
    }
    #[test] fn combined_mode_disagreement_and_missing_rpm_stop_control(){
        for missing_rpm in [true,false]{
            let mut io=fake();let mut s=Session::start(config(),&mut io).unwrap();tick(&mut s,&mut io,0);
            if missing_rpm{io.snapshot.cpu_fan_rpm=None;}else{
                io.snapshot.reads[2].output.insert("gmOutput".into(),RawValue::unsigned(0x4100));
            }
            tick(&mut s,&mut io,1000);assert_eq!(io.writes,1);assert_eq!(io.restores,1);assert!(!s.active());
        }
    }
    #[test] fn future_sample_is_rejected_even_during_pair_limit(){
        let mut io=fake();let mut s=Session::start(config(),&mut io).unwrap();tick(&mut s,&mut io,0);
        let sample=io.read();s.tick_sample(501,500,sample,&mut io);assert_eq!(io.writes,1);assert_eq!(io.restores,1);
    }
}
pub(crate) trait ThermalIo {
    fn ready(&mut self) -> Result<bool, DeviceError>;
    fn read(&mut self) -> Result<Snapshot, DeviceError>;
    fn write(&mut self, cpu: u8, gpu: u8) -> Result<ApplyReport, DeviceError>;
    fn restore_auto(&mut self) -> Result<ApplyReport, DeviceError>;
}
pub(crate) struct Session {
    config: ThermalConfig,
    cpu: Engine,
    gpu: Engine,
    origin: Instant,
    last_tick: Option<u64>,
    last_pair_write: Option<u64>,
    status: ThermalStatus,
}
fn invalid(message: impl Into<String>) -> DeviceError { DeviceError {kind:ErrorKind::InvalidArgument,message:message.into()} }
impl Session {
    pub fn start(config: ThermalConfig, io: &mut impl ThermalIo) -> Result<Self,DeviceError> {
        let cpu=Engine::new(config.cpu.clone()).map_err(invalid)?;
        let gpu=Engine::new(config.gpu.clone()).map_err(invalid)?;
        if !io.ready()? { return Err(invalid("Verified custom-fan capability is required")); }
        let snapshot=io.read()?;
        // DC support is not inferred from AC tests. Start itself performs no write.
        if snapshot.ac_online!=Some(true) || snapshot.cpu_temperature_c.is_none() || snapshot.gpu_temperature_c.is_none() || !fan_control_ready(&snapshot) {
            return Err(invalid("Fresh CPU/GPU temperatures, AC power and known fan state required"));
        }
        Ok(Self {config,cpu,gpu,origin:Instant::now(),last_tick:None,last_pair_write:None,status:ThermalStatus {active:true,message:"Curve armed; awaiting first sample".into(),..Default::default()} })
    }
    pub fn status(&self) -> ThermalStatus { self.status.clone() }
    pub fn active(&self) -> bool { self.status.active }
    pub fn owns_fans(&self) -> bool { self.status.active || self.status.restore_pending }
    pub fn tick(&mut self, io: &mut impl ThermalIo) {
        if !self.status.active { return; }
        let started=self.origin.elapsed().as_millis() as u64;
        let snapshot=io.read();
        let now=self.origin.elapsed().as_millis() as u64;
        self.tick_sample(started,now,snapshot,io);
    }
    fn tick_sample(&mut self, captured: u64, now: u64, snapshot: Result<Snapshot,DeviceError>, io: &mut impl ThermalIo) {
        if !self.status.active { return; }
        let age_limit=self.config.cpu.maximum_sample_age_ms.min(self.config.gpu.maximum_sample_age_ms);
        if captured>now || self.last_tick.is_some_and(|last| now<last || now-last>age_limit) {
            self.stop(io,"Sampling interrupted; explicit restart required"); return;
        }
        self.last_tick=Some(now);
        let snapshot=match snapshot {Ok(s)=>s,Err(e)=>{self.stop(io,&format!("Temperature read failed: {}",e.message));return;}};
        if snapshot.ac_online!=Some(true) { self.stop(io,"AC power unavailable; curve stopped");return; }
        if !fan_control_ready(&snapshot) {self.stop(io,"Independent fan state or RPM readback unavailable; curve stopped");return;}
        if let (Some(cpu),Some(gpu))=(self.status.cpu_target,self.status.gpu_target) {
            if snapshot.fan_mode!=Some(FanMode::Custom) || snapshot.cpu_fan_percent!=Some(cpu) || snapshot.gpu_fan_percent!=Some(gpu) {
                self.stop(io,"Another controller changed fan state; curve stopped");return;
            }
        }
        // Both setters are sent together; enforce the slower fan's interval
        // for the entire pair, including when only one target changes.
        let interval=self.config.cpu.minimum_write_interval_ms.max(self.config.gpu.minimum_write_interval_ms);
        let pair_limited=self.last_pair_write.is_some_and(|last|now.saturating_sub(last)<interval);
        // Invalid samples must still cause fallback during the rate-limit window.
        if pair_limited {
            if now.saturating_sub(captured)>age_limit || [snapshot.cpu_temperature_c,snapshot.gpu_temperature_c].iter().any(|v|!v.is_some_and(|t|t.is_finite()&&(1.0..=125.0).contains(&t))) {
                self.stop(io,"Invalid or stale temperature during write interval");
            }
            return;
        }
        let cpu=self.cpu.observe(now,Sample{celsius:snapshot.cpu_temperature_c.map(f64::from),captured_ms:captured});
        let gpu=self.gpu.observe(now,Sample{celsius:snapshot.gpu_temperature_c.map(f64::from),captured_ms:captured});
        if matches!(cpu,Decision::RestoreAuto(_)) || matches!(gpu,Decision::RestoreAuto(_)) {
            self.stop(io,"Invalid or stale temperature; curve stopped");return;
        }
        if cpu==Decision::Hold && gpu==Decision::Hold {return;}
        let cpu_target=match cpu {Decision::SetPercent(v)=>Some(v),_=>self.status.cpu_target};
        let gpu_target=match gpu {Decision::SetPercent(v)=>Some(v),_=>self.status.gpu_target};
        let (Some(cpu_target),Some(gpu_target))=(cpu_target,gpu_target) else {self.stop(io,"Incomplete paired target");return;};
        self.last_pair_write=Some(now);
        let report=match io.write(cpu_target,gpu_target) {Ok(r)=>r,Err(e)=>{self.stop(io,&format!("Fan write failed: {}",e.message));return;}};
        let confirmed=report.status==ApplyStatus::Applied && report.after.as_ref().is_some_and(|s|s.fan_mode==Some(FanMode::Custom)&&s.cpu_fan_percent==Some(cpu_target)&&s.gpu_fan_percent==Some(gpu_target));
        self.status.last_report=Some(report);
        if !confirmed {self.stop(io,"Fan target readback not confirmed");return;}
        self.cpu.complete(Some(cpu_target)); self.gpu.complete(Some(gpu_target));
        self.status.cpu_target=Some(cpu_target);self.status.gpu_target=Some(gpu_target);
        self.status.message="Software curve active; targets read back".into();
    }
    pub fn stop(&mut self, io: &mut impl ThermalIo, reason: &str) {
        if !self.owns_fans() {return;}
        self.status.active=false;
        self.status.restore_pending=true;
        match io.restore_auto() {
            Ok(report)=> {
                let confirmed=report.status==ApplyStatus::Applied && report.after.as_ref().is_some_and(|s|s.fan_mode==Some(FanMode::Auto));
                self.status.last_report=Some(report);self.status.restore_pending=!confirmed;
                self.status.message=format!("{reason}; {}",if confirmed {"Auto restoration confirmed"}else{"Auto restoration NOT confirmed"});
            }
            Err(e)=>self.status.message=format!("{reason}; Auto restoration failed: {}",e.message),
        }
    }
}
