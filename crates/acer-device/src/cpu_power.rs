//! Supervised CPU power transaction orchestration. Native IPF transport is separate.
//! Backends must return primitive readbacks, never substituted capability maxima.
use acer_protocol::ipf_power::{Limit, Operation};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Error { pub operation: String, pub code: Option<u32>, pub message: String }
impl Error {
    fn invalid(message: &str) -> Self { Self { operation: "preflight".into(), code: None, message: message.into() } }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct State { pub power_mw: u32, pub time_ms: Option<i64>, pub enabled: bool }
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Capability {
    pub min_mw: u32, pub max_mw: u32,
    /// None means the time-window capability has not been established.
    pub time_bounds_ms: Option<(i64, i64)>,
}
impl Capability {
    fn power_valid(&self, v: u32) -> bool { self.min_mw > 0 && self.min_mw <= self.max_mw && (self.min_mw..=self.max_mw).contains(&v) }
    fn time_valid(&self, v: i64) -> bool { self.time_bounds_ms.is_some_and(|(min,max)| min >= 0 && min <= max && (min..=max).contains(&v)) }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Change { pub limit: Limit, pub power_mw: u32, pub time_ms: Option<i64> }
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint { pub limit: Limit, pub original: State, pub target: State, pub capability: Capability }
pub trait CpuIo {
    fn read(&mut self, limit: Limit) -> Result<State, Error>;
    fn capability(&mut self, limit: Limit) -> Result<Capability, Error>;
    /// Raw SET primitive only; implementations must not silently enable other fields.
    fn write(&mut self, limit: Limit, operation: Operation) -> Result<(), Error>;
}
#[derive(Debug, Serialize)]
pub struct WriteResult { pub limit: Limit, pub operation: Operation, pub result: Result<(), Error> }
#[derive(Debug, Serialize)]
pub struct ReadResult { pub limit: Limit, pub state: Result<State, Error> }
#[derive(Debug, Serialize)]
pub struct Report {
    pub checkpoints: Vec<Checkpoint>,
    pub apply_writes: Vec<WriteResult>, pub apply_reads: Vec<ReadResult>,
    pub restore_writes: Vec<WriteResult>, pub restore_reads: Vec<ReadResult>,
}
impl Report {
    pub fn success(&self) -> bool {
        !self.checkpoints.is_empty()
            && self.apply_writes.iter().all(|v| v.result.is_ok())
            && self.restore_writes.iter().all(|v| v.result.is_ok())
            && self.checkpoints.iter().all(|p| self.apply_reads.iter().any(|r| r.limit == p.limit && r.state.as_ref() == Ok(&p.target)))
            && self.checkpoints.iter().all(|p| self.restore_reads.iter().any(|r| r.limit == p.limit && r.state.as_ref() == Ok(&p.original)))
    }
}
fn record_write(io: &mut impl CpuIo, records: &mut Vec<WriteResult>, limit: Limit, operation: Operation) -> bool {
    let result = io.write(limit, operation); let ok = result.is_ok();
    records.push(WriteResult { limit, operation, result }); ok
}
/// Persist all original fields before ANY setter. Only explicitly requested, enabled
/// PLs are changed. No automatic enabling, guessed ranges or default restore values.
pub fn trial(io: &mut impl CpuIo, changes: &[Change],
    mut persist: impl FnMut(&[Checkpoint]) -> Result<(), Error>) -> Result<Report, Error> {
    if changes.is_empty() || changes.len() > 2 { return Err(Error::invalid("One or two distinct PL targets are required")); }
    let mut points: Vec<Checkpoint> = Vec::new();
    for c in changes {
        if points.iter().any(|p| p.limit == c.limit) { return Err(Error::invalid("Duplicate PL target")); }
        let cap = io.capability(c.limit)?;
        let original = io.read(c.limit)?;
        if !original.enabled { return Err(Error::invalid("Disabled PL requires a separately designed enable experiment")); }
        if !cap.power_valid(c.power_mw) || !cap.power_valid(original.power_mw) { return Err(Error::invalid("Power target or original outside confirmed dynamic range")); }
        if let Some(t) = c.time_ms {
            if !cap.time_valid(t) || !original.time_ms.is_some_and(|v| cap.time_valid(v)) {
                return Err(Error::invalid("Time-window support, target or original not established"));
            }
        }
        let target = State { power_mw: c.power_mw, time_ms: c.time_ms.or(original.time_ms), enabled: original.enabled };
        if original == target { return Err(Error::invalid("Target makes no change")); }
        points.push(Checkpoint { limit: c.limit, original, target, capability: cap });
    }
    persist(&points)?;
    // Reject a changed baseline after persistence, before issuing any writes.
    for p in &points {
        if io.read(p.limit)? != p.original { return Err(Error::invalid("Baseline changed while preparing trial")); }
    }
    let mut report = Report { checkpoints: points, apply_writes: vec![], apply_reads: vec![], restore_writes: vec![], restore_reads: vec![] };
    // Track attempted fields, including a setter which returned an error after mutation.
    let mut attempted: Vec<(usize, bool, bool)> = Vec::new();
    for (idx,p) in report.checkpoints.iter().enumerate() {
        let mut power_attempt = false; let mut time_attempt = false; let mut ok = true;
        if p.original.power_mw != p.target.power_mw {
            power_attempt = true;
            ok = record_write(io, &mut report.apply_writes, p.limit, Operation::SetLimitMw(p.target.power_mw));
        }
        if ok && p.original.time_ms != p.target.time_ms {
            time_attempt = true;
            ok = record_write(io, &mut report.apply_writes, p.limit, Operation::SetTimeWindowMs(p.target.time_ms.expect("preflight")));
        }
        attempted.push((idx,power_attempt,time_attempt));
        let state = io.read(p.limit);
        ok &= state.as_ref() == Ok(&p.target);
        report.apply_reads.push(ReadResult { limit: p.limit, state });
        if !ok { break; }
    }
    // Restore touched data first, then original enables. Never stop restoration after
    // one failure, and never issue setters for a PL that was not attempted.
    for &(idx,power,time) in attempted.iter().rev() {
        let p = &report.checkpoints[idx];
        if time { record_write(io, &mut report.restore_writes, p.limit, Operation::SetTimeWindowMs(p.original.time_ms.expect("preflight"))); }
        if power { record_write(io, &mut report.restore_writes, p.limit, Operation::SetLimitMw(p.original.power_mw)); }
    }
    for &(idx,_,_) in attempted.iter().rev() {
        let p = &report.checkpoints[idx];
        record_write(io, &mut report.restore_writes, p.limit, Operation::SetEnabled(p.original.enabled));
    }
    for p in &report.checkpoints { report.restore_reads.push(ReadResult { limit: p.limit, state: io.read(p.limit) }); }
    Ok(report)
}

#[derive(Debug, Serialize)]
pub struct RecoveryReport { pub originals: Vec<Checkpoint>, pub writes: Vec<WriteResult>, pub readbacks: Vec<ReadResult> }
impl RecoveryReport {
    pub fn success(&self)->bool {
        !self.originals.is_empty() && self.writes.iter().all(|w|w.result.is_ok())
            && self.originals.iter().all(|p|self.readbacks.iter().any(|r|r.limit==p.limit && r.state.as_ref()==Ok(&p.original)))
    }
}
/// Recovery from a durable checkpoint after interruption. Caller must bind device
/// identity before this function. Revalidate every saved original against current
/// capabilities before any setter; do not require the interrupted target to match.
pub fn recover(io:&mut impl CpuIo,points:&[Checkpoint])->Result<RecoveryReport,Error> {
    if points.is_empty() || points.len()>2 {return Err(Error::invalid("One or two recovery checkpoints required"));}
    let mut before=Vec::new();
    for (index,p) in points.iter().enumerate() {
        if points[..index].iter().any(|v|v.limit==p.limit) {return Err(Error::invalid("Duplicate recovery PL"));}
        let cap=io.capability(p.limit)?;
        if !cap.power_valid(p.original.power_mw) {return Err(Error::invalid("Original power outside current capability"));}
        if p.original.time_ms!=p.target.time_ms && !p.original.time_ms.is_some_and(|v|cap.time_valid(v)) {return Err(Error::invalid("Original time outside current capability"));}
        before.push(io.read(p.limit)?);
    }
    let mut report=RecoveryReport {originals:points.to_vec(),writes:vec![],readbacks:vec![]};
    for (p,current) in points.iter().zip(&before).rev() {
        if let Some(time)=p.original.time_ms.filter(|_|p.original.time_ms!=p.target.time_ms) {
            if current.time_ms!=Some(time) {record_write(io,&mut report.writes,p.limit,Operation::SetTimeWindowMs(time));}
        }
        if p.original.power_mw!=p.target.power_mw && current.power_mw!=p.original.power_mw {record_write(io,&mut report.writes,p.limit,Operation::SetLimitMw(p.original.power_mw));}
    }
    // Reassert saved enables after any field writes; setters may have side effects.
    for (p,current) in points.iter().zip(&before).rev() {
        if current!=&p.original {record_write(io,&mut report.writes,p.limit,Operation::SetEnabled(p.original.enabled));}
    }
    for p in points {report.readbacks.push(ReadResult {limit:p.limit,state:io.read(p.limit)});}
    Ok(report)
}

#[cfg(test)] mod tests {
    use super::*;
    struct Fake { states: [State;2], writes: Vec<(Limit,Operation)>, fail_at: Option<usize>, fail_after_write: bool, clamp: bool }
    impl Default for Fake { fn default() -> Self { Self { states: [State { power_mw:45000,time_ms:Some(28000),enabled:true }, State { power_mw:60000,time_ms:None,enabled:true }], writes:vec![],fail_at:None,fail_after_write:false,clamp:false } } }
    fn index(l: Limit) -> usize { if l == Limit::Pl1 {0} else {1} }
    fn changes() -> Vec<Change> { vec![Change {limit:Limit::Pl1,power_mw:40000,time_ms:Some(20000)},Change {limit:Limit::Pl2,power_mw:55000,time_ms:None}] }
    impl CpuIo for Fake {
        fn read(&mut self,l:Limit)->Result<State,Error>{Ok(self.states[index(l)].clone())}
        fn capability(&mut self,l:Limit)->Result<Capability,Error>{Ok(Capability {min_mw:10000,max_mw:100000,time_bounds_ms:if l==Limit::Pl1 {Some((1000,50000))}else{None}})}
        fn write(&mut self,l:Limit,op:Operation)->Result<(),Error>{
            self.writes.push((l,op));let fail=self.fail_at==Some(self.writes.len());
            if fail&&!self.fail_after_write{return Err(Error::invalid("injected write failure"));}
            let s=&mut self.states[index(l)];match op {
                Operation::SetLimitMw(v)=>s.power_mw=if self.clamp&&v==40000 {39000}else{v},
                Operation::SetTimeWindowMs(v)=>s.time_ms=Some(v),Operation::SetEnabled(v)=>s.enabled=v,_=>panic!("setter only"),
            }
            if fail {Err(Error::invalid("injected failure after mutation"))}else{Ok(())}
        }
    }
    #[test] fn full_trial_restores_data_then_enables() {
        let mut io=Fake::default();let original=io.states.clone();let mut saved=vec![];
        let r=trial(&mut io,&changes(),|p|{saved=p.to_vec();Ok(())}).unwrap();
        assert!(r.success());assert_eq!(io.states,original);assert_eq!(saved.len(),2);
        let ops:Vec<_>=r.restore_writes.iter().map(|w|w.operation).collect();
        assert_eq!(ops,[Operation::SetLimitMw(60000),Operation::SetTimeWindowMs(28000),Operation::SetLimitMw(45000),Operation::SetEnabled(true),Operation::SetEnabled(true)]);
    }
    #[test] fn failed_second_field_restores_partial_change_and_never_touches_pl2() {
        let mut io=Fake {fail_at:Some(2),fail_after_write:true,..Fake::default()};let original=io.states.clone();
        let r=trial(&mut io,&changes(),|_|Ok(())).unwrap();
        assert!(!r.success());assert_eq!(io.states,original);assert!(io.writes.iter().all(|(l,_)|*l==Limit::Pl1));
    }
    #[test] fn restore_failure_does_not_skip_later_restore_fields() {
        let mut io=Fake {fail_at:Some(4),..Fake::default()};
        let r=trial(&mut io,&changes(),|_|Ok(())).unwrap();
        assert!(!r.success());assert_eq!(r.restore_writes.len(),5);assert!(r.restore_writes[0].result.is_err());
        assert_eq!(io.states[0],Fake::default().states[0]);
    }
    #[test] fn missing_time_capability_and_persistence_failure_never_write() {
        let mut io=Fake::default();let bad=[Change {limit:Limit::Pl2,power_mw:55000,time_ms:Some(1000)}];
        assert!(trial(&mut io,&bad,|_|panic!("preflight should fail")).is_err());
        assert!(trial(&mut io,&changes(),|_|Err(Error::invalid("disk full"))).is_err());assert!(io.writes.is_empty());
    }
    #[test] fn disabled_or_duplicate_targets_are_not_silently_enabled() {
        let mut io=Fake::default();io.states[0].enabled=false;
        assert!(trial(&mut io,&changes(),|_|Ok(())).is_err());assert!(io.writes.is_empty());
        let mut io=Fake::default();let c=changes()[0].clone();assert!(trial(&mut io,&[c.clone(),c],|_|Ok(())).is_err());assert!(io.writes.is_empty());
    }
    #[test] fn clamped_readback_stops_before_next_pl_and_restores() {
        let mut io=Fake {clamp:true,..Fake::default()};let r=trial(&mut io,&changes(),|_|Ok(())).unwrap();
        assert!(!r.success());assert_eq!(io.states,Fake::default().states);assert!(io.writes.iter().all(|(l,_)|*l==Limit::Pl1));
    }
    fn checkpoints()->Vec<Checkpoint> {
        let mut io=Fake::default();let mut saved=vec![];
        trial(&mut io,&changes(),|p|{saved=p.to_vec();Ok(())}).unwrap();saved
    }
    #[test] fn interrupted_trial_can_restore_from_saved_originals() {
        let saved=checkpoints();let mut io=Fake::default();
        io.states[0]=saved[0].target.clone();io.states[0].enabled=false;
        io.states[1]=saved[1].target.clone();
        let result=recover(&mut io,&saved).unwrap();assert!(result.success());
        assert_eq!(io.states,Fake::default().states);
        io.writes.clear();assert!(recover(&mut io,&saved).unwrap().success());assert!(io.writes.is_empty());
    }
    #[test] fn recovery_validates_all_limits_before_any_setter() {
        let mut saved=checkpoints();saved[1].original.power_mw=200000;
        let mut io=Fake::default();io.states[0].power_mw=40000;
        assert!(recover(&mut io,&saved).is_err());assert!(io.writes.is_empty());
    }
    #[test] fn recovery_failure_is_visible_and_does_not_skip_other_fields() {
        let saved=checkpoints();let mut io=Fake {fail_at:Some(1),..Fake::default()};
        io.states[0]=saved[0].target.clone();io.states[1]=saved[1].target.clone();
        let result=recover(&mut io,&saved).unwrap();assert!(!result.success());
        assert_eq!(io.states[0],saved[0].original);assert!(result.writes.len()>1);
    }
    #[test] fn recovery_does_not_write_time_when_trial_only_changed_power() {
        let mut io=Fake::default();let mut saved=vec![];
        trial(&mut io,&[Change {limit:Limit::Pl1,power_mw:40000,time_ms:None}],|p|{saved=p.to_vec();Ok(())}).unwrap();
        io.writes.clear();io.states[0].power_mw=40000;io.states[0].time_ms=Some(30000);
        let r=recover(&mut io,&saved).unwrap();
        assert!(!r.success());assert_eq!(io.states[0].power_mw,45000);
        assert_eq!(io.states[0].time_ms,Some(30000));
        assert!(!io.writes.iter().any(|(_,op)|matches!(op,Operation::SetTimeWindowMs(_))));
    }
}
