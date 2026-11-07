//! Pure software-curve decisions. No hardware access or assumed firmware thresholds.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Point { pub celsius: f64, pub percent: u8 }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Curve {
    pub points: Vec<Point>,
    pub hysteresis_celsius: f64,
    pub minimum_write_interval_ms: u64,
    pub maximum_sample_age_ms: u64,
}
impl Curve {
    /// Bounds describe representable software settings, not safe machine limits.
    /// Runtime must additionally enforce the device's verified capability envelope.
    pub fn validate(&self) -> Result<(), &'static str> {
        if !(2..=16).contains(&self.points.len()) { return Err("Expected 2–16 curve points"); }
        if !self.hysteresis_celsius.is_finite() || !(0.0..=10.0).contains(&self.hysteresis_celsius) { return Err("Invalid hysteresis"); }
        if !(500..=30_000).contains(&self.minimum_write_interval_ms) { return Err("Invalid write interval"); }
        if !(500..=30_000).contains(&self.maximum_sample_age_ms) { return Err("Invalid sample age"); }
        for (i, p) in self.points.iter().enumerate() {
            if !p.celsius.is_finite() || !(1.0..=125.0).contains(&p.celsius) || p.percent > 100 { return Err("Invalid curve point"); }
            if i > 0 && (p.celsius <= self.points[i-1].celsius || p.percent < self.points[i-1].percent) { return Err("Curve must have increasing temperatures and nondecreasing speeds"); }
        }
        Ok(())
    }
    fn interpolate(&self, celsius: f64) -> u8 {
        if celsius <= self.points[0].celsius { return self.points[0].percent; }
        for pair in self.points.windows(2) {
            let (a,b)=(&pair[0],&pair[1]);
            if celsius <= b.celsius {
                let value = f64::from(a.percent) + (celsius-a.celsius)/(b.celsius-a.celsius)*f64::from(b.percent-a.percent);
                return value.ceil() as u8;
            }
        }
        self.points.last().unwrap().percent
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Sample { pub celsius: Option<f64>, pub captured_ms: u64 }
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault { MissingTemperature, InvalidTemperature, StaleSample, ClockRollback, WriteUnconfirmed }
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision { Hold, SetPercent(u8), RestoreAuto(Fault) }

/// One instance per fan. Caller owns a monotonic clock and serial write/readback.
/// A fault is latched: reconstruct only after explicit restart and successful fallback.
pub struct Engine {
    curve: Curve,
    last_now: Option<u64>,
    last_sample: Option<u64>,
    last_write: Option<u64>,
    confirmed: Option<u8>,
    pending: Option<u8>,
    fault: Option<Fault>,
}
impl Engine {
    pub fn new(curve: Curve) -> Result<Self, &'static str> {
        curve.validate()?;
        Ok(Self { curve, last_now: None, last_sample: None, last_write: None, confirmed: None, pending: None, fault: None })
    }
    fn fail(&mut self, fault: Fault) -> Decision {
        self.fault=Some(fault); self.pending=None; Decision::RestoreAuto(fault)
    }
    pub fn observe(&mut self, now_ms: u64, sample: Sample) -> Decision {
        if let Some(fault)=self.fault { return Decision::RestoreAuto(fault); }
        if self.last_now.is_some_and(|last| now_ms < last) || sample.captured_ms > now_ms || self.last_sample.is_some_and(|last| sample.captured_ms < last) { return self.fail(Fault::ClockRollback); }
        self.last_now=Some(now_ms);
        if now_ms-sample.captured_ms > self.curve.maximum_sample_age_ms { return self.fail(Fault::StaleSample); }
        let Some(temp)=sample.celsius else { return self.fail(Fault::MissingTemperature); };
        if !temp.is_finite() || !(1.0..=125.0).contains(&temp) { return self.fail(Fault::InvalidTemperature); }
        if self.last_sample == Some(sample.captured_ms) { return Decision::Hold; }
        self.last_sample=Some(sample.captured_ms);
        if self.pending.is_some() { return Decision::Hold; }
        let rising=self.curve.interpolate(temp);
        let target=match self.confirmed {
            Some(current) if rising < current => self.curve.interpolate(temp+self.curve.hysteresis_celsius).min(current),
            _ => rising,
        };
        if self.confirmed==Some(target) || self.last_write.is_some_and(|last| now_ms-last < self.curve.minimum_write_interval_ms) { return Decision::Hold; }
        self.pending=Some(target); self.last_write=Some(now_ms);
        Decision::SetPercent(target)
    }
    /// Only exact hardware readback completes a request. An ACK alone is insufficient.
    pub fn complete(&mut self, readback: Option<u8>) -> Decision {
        let Some(target)=self.pending.take() else { return self.fault.map(Decision::RestoreAuto).unwrap_or(Decision::Hold); };
        if readback != Some(target) { return self.fail(Fault::WriteUnconfirmed); }
        self.confirmed=Some(target); Decision::Hold
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn curve() -> Curve { Curve { points:vec![Point{celsius:40.0,percent:30},Point{celsius:80.0,percent:100}],hysteresis_celsius:3.0,minimum_write_interval_ms:1000,maximum_sample_age_ms:2000 } }
    fn sample(t: f64, ms: u64) -> Sample { Sample {celsius:Some(t),captured_ms:ms} }
    #[test] fn rejects_malformed_curves() {
        let mut c=curve(); c.points[1].celsius=40.0; assert!(c.validate().is_err());
        c=curve(); c.points[1].percent=20; assert!(c.validate().is_err());
        c=curve(); c.hysteresis_celsius=f64::NAN; assert!(c.validate().is_err());
    }
    #[test] fn interpolation_and_endpoint_clamping() {
        let c=curve(); assert_eq!(c.interpolate(10.0),30); assert_eq!(c.interpolate(60.0),65); assert_eq!(c.interpolate(100.0),100);
    }
    #[test] fn rate_limit_and_hysteresis() {
        let mut e=Engine::new(curve()).unwrap();
        assert_eq!(e.observe(0,sample(60.0,0)),Decision::SetPercent(65)); e.complete(Some(65));
        assert_eq!(e.observe(500,sample(70.0,500)),Decision::Hold);
        assert_eq!(e.observe(1000,sample(59.0,1000)),Decision::Hold);
        assert_eq!(e.observe(2000,sample(55.0,2000)),Decision::SetPercent(62));
    }
    #[test] fn duplicate_sample_does_not_replay_and_stale_sample_falls_back() {
        let mut e=Engine::new(curve()).unwrap(); e.observe(0,sample(60.0,0)); e.complete(Some(65));
        assert_eq!(e.observe(1000,sample(60.0,0)),Decision::Hold);
        assert_eq!(e.observe(2001,sample(60.0,0)),Decision::RestoreAuto(Fault::StaleSample));
        assert_eq!(e.observe(3000,sample(60.0,3000)),Decision::RestoreAuto(Fault::StaleSample));
    }
    #[test] fn failed_readback_latches_fault() {
        let mut e=Engine::new(curve()).unwrap(); e.observe(0,sample(60.0,0));
        assert_eq!(e.complete(Some(64)),Decision::RestoreAuto(Fault::WriteUnconfirmed));
    }
    #[test] fn invalid_temperature_and_clock_restore() {
        for temp in [0.0,f64::NAN,f64::INFINITY,126.0] {
            let mut e=Engine::new(curve()).unwrap(); assert_eq!(e.observe(0,sample(temp,0)),Decision::RestoreAuto(Fault::InvalidTemperature));
        }
        let mut e=Engine::new(curve()).unwrap(); assert_eq!(e.observe(0,sample(50.0,1)),Decision::RestoreAuto(Fault::ClockRollback));
        let mut e=Engine::new(curve()).unwrap(); assert_eq!(e.observe(0,Sample{celsius:None,captured_ms:0}),Decision::RestoreAuto(Fault::MissingTemperature));
    }
}
