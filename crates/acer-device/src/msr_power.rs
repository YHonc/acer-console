//! MSR transaction orchestration, with no native transport in this module.
//! The caller must establish machine identity, supported registers and policy.
use crate::cpu_power::Error;
use acer_protocol::rapl_plan::{self,Target};
use serde::{Deserialize,Serialize};

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Snapshot {pub units:u64,pub limits:u64}
pub trait MsrIo {
    fn read(&mut self)->Result<Snapshot,Error>;
    /// Only MSR_PKG_POWER_LIMIT (0x610), never an arbitrary register selector.
    /// Re-read and reject a changed full register; not an atomic hardware CAS.
    fn write_limits(&mut self,expected:u64,value:u64)->Result<(),Error>;
}
#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct Checkpoint {pub units_hex:String,pub original_hex:String,pub target_hex:String,pub mask_hex:String}
#[derive(Debug,Serialize)]
pub struct Report {
    pub checkpoint:Checkpoint,
    pub apply_error:Option<Error>,pub applied_hex:Result<String,Error>,
    pub restore_error:Option<Error>,pub restored_hex:Result<String,Error>,
    pub success:bool,
}
#[derive(Debug,Serialize)]
pub struct RecoveryReport {pub checkpoint:Checkpoint,pub error:Option<Error>,pub readback_hex:Result<String,Error>,pub success:bool}
fn local(op:&str,message:impl Into<String>)->Error {Error {operation:op.into(),code:None,message:message.into()}}
fn hex(v:u64)->String {format!("0x{v:016X}")}
fn parse(v:&str)->Result<u64,Error>{
    if v.len()!=18 || !v.starts_with("0x") {return Err(local("checkpoint","Expected exact 64-bit hexadecimal string"));}
    u64::from_str_radix(&v[2..],16).map_err(|e|local("checkpoint",e.to_string()))
}
/// Trial scope: existing enabled limits may be reduced, not raised. This is a
/// local experiment policy, NOT evidence of a firmware-advertised safe range.
fn reduction_only(before:u64,after:u64)->bool {
    [0,32].into_iter().all(|shift|((after>>shift)&0x7fff)<=((before>>shift)&0x7fff))
}
fn restore(io:&mut impl MsrIo,checkpoint:&Checkpoint)->(Option<Error>,Result<String,Error>) {
    let operation=(||{
        let units=parse(&checkpoint.units_hex)?;let original=parse(&checkpoint.original_hex)?;
        let target=parse(&checkpoint.target_hex)?;let mask=parse(&checkpoint.mask_hex)?;
        let current=io.read()?;
        if current.units!=units{return Err(local("restore","RAPL units changed"));}
        let value=rapl_plan::restore_value(original,target,mask,current.limits).map_err(|e|local("restore",format!("{e:?}")))?;
        if value!=current.limits{io.write_limits(current.limits,value)?;}
        Ok(())
    })();
    let readback=io.read().and_then(|s|{
        if hex(s.units)!=checkpoint.units_hex{return Err(local("restore_readback","RAPL units changed"));}
        Ok(hex(s.limits))
    });
    (operation.err(),readback)
}
pub fn recover(io:&mut impl MsrIo,checkpoint:&Checkpoint)->Result<RecoveryReport,Error>{
    let units=parse(&checkpoint.units_hex)?;let original=parse(&checkpoint.original_hex)?;
    let target=parse(&checkpoint.target_hex)?;let mask=parse(&checkpoint.mask_hex)?;
    // Rebuild the plan to reject malformed/tampered field masks or flag edits.
    let decoded=acer_protocol::rapl::decode(Some(units),Some(target)).ok_or_else(||local("checkpoint","Missing values"))?;
    let mw=|raw:u64|->Result<u32,Error>{
        let n=raw*1000;let d=1u64<<(units&15);
        if n%d!=0{return Err(local("checkpoint","Non-integer milliwatt target"));}Ok((n/d) as u32)
    };
    let time_ms=if mask&(0x7f<<17)!=0 {
        let ms=decoded.pl1.window_seconds*1000.0;
        if ms.fract()!=0.0 || ms>u64::MAX as f64{return Err(local("checkpoint","Non-integer millisecond target"));}Some(ms as u64)
    }else{None};
    let request=Target {pl1_mw:if mask&0x7fff!=0{Some(mw(target&0x7fff)?)}else{None},
        pl2_mw:if mask&(0x7fff<<32)!=0{Some(mw((target>>32)&0x7fff)?)}else{None},pl1_time_ms:time_ms};
    let rebuilt=rapl_plan::prepare(units,original,request).map_err(|e|local("checkpoint",format!("{e:?}")))?;
    if rebuilt.target_hex!=checkpoint.target_hex || rebuilt.mask_hex!=checkpoint.mask_hex || !reduction_only(original,target) {
        return Err(local("checkpoint","Checkpoint does not describe an allowed trial"));
    }
    let (error,readback_hex)=restore(io,checkpoint);
    let success=error.is_none() && readback_hex.as_ref().ok().and_then(|s|parse(s).ok()).is_some_and(|v|v&mask==original&mask);
    Ok(RecoveryReport {checkpoint:checkpoint.clone(),error,readback_hex,success})
}
pub fn trial(io:&mut impl MsrIo,target:Target,mut persist:impl FnMut(&Checkpoint)->Result<(),Error>)->Result<Report,Error>{
    let before=io.read()?;
    let plan=rapl_plan::prepare(before.units,before.limits,target).map_err(|e|local("preflight",format!("{e:?}")))?;
    let next=parse(&plan.target_hex)?;let mask=parse(&plan.mask_hex)?;
    if !reduction_only(before.limits,next){return Err(local("preflight","This experiment permits power reductions only"));}
    let checkpoint=Checkpoint {units_hex:plan.units_hex,original_hex:plan.original_hex,target_hex:plan.target_hex,mask_hex:plan.mask_hex};
    persist(&checkpoint)?;
    if io.read()?!=before{return Err(local("preflight","Baseline changed after persistence; no setter called"));}
    let apply_error=io.write_limits(before.limits,next).err();
    let applied=io.read().and_then(|s|{
        if s.units!=before.units{return Err(local("apply_readback","RAPL units changed"));}Ok(s.limits)
    });
    let (restore_error,restored_hex)=restore(io,&checkpoint);
    let restored_matches=restored_hex.as_ref().ok().and_then(|s|parse(s).ok()).is_some_and(|v|v&mask==before.limits&mask);
    let success=apply_error.is_none() && applied.as_ref()==Ok(&next) && restore_error.is_none() && restored_matches;
    Ok(Report {checkpoint,apply_error,applied_hex:applied.map(hex),restore_error,restored_hex,success})
}

#[cfg(test)]mod tests{
    use super::*;
    const UNITS:u64=0x000A0E03;const ORIGINAL:u64=0x004284E800DF8320;
    struct Fake {s:Snapshot,writes:Vec<u64>,fail_first:bool,fail_restore:bool,external:bool}
    impl Default for Fake {fn default()->Self{Self{s:Snapshot{units:UNITS,limits:ORIGINAL},writes:vec![],fail_first:false,fail_restore:false,external:false}}}
    impl MsrIo for Fake {
        fn read(&mut self)->Result<Snapshot,Error>{Ok(self.s)}
        fn write_limits(&mut self,expected:u64,v:u64)->Result<(),Error>{
            if self.s.limits!=expected{return Err(local("write","Concurrent change before setter"));}
            self.writes.push(v);
            if self.fail_restore && self.writes.len()>1{return Err(local("write","restore failed"));}
            self.s.limits=v;
            if self.external && self.writes.len()==1{self.s.limits=(v&!0x7fff)|500;}
            if self.fail_first && self.writes.len()==1{return Err(local("write","failed after mutation"));}
            Ok(())
        }
    }
    fn target()->Target{Target{pl1_mw:Some(75000),..Target::default()}}
    #[test]fn save_apply_read_restore(){
        let mut io=Fake::default();let mut saved=None;
        let r=trial(&mut io,target(),|p|{saved=Some(p.clone());Ok(())}).unwrap();
        assert!(r.success);assert_eq!(io.s.limits,ORIGINAL);assert_eq!(io.writes.len(),2);assert!(saved.is_some());
    }
    #[test]fn write_error_after_mutation_still_restores(){
        let mut io=Fake{fail_first:true,..Fake::default()};let r=trial(&mut io,target(),|_|Ok(())).unwrap();
        assert!(!r.success);assert!(r.apply_error.is_some());assert_eq!(io.s.limits,ORIGINAL);
    }
    #[test]fn restore_failure_retains_original_and_error(){
        let mut io=Fake{fail_restore:true,..Fake::default()};let r=trial(&mut io,target(),|_|Ok(())).unwrap();
        assert!(!r.success);assert!(r.restore_error.is_some());assert_eq!(r.checkpoint.original_hex,hex(ORIGINAL));
    }
    #[test]fn owned_field_external_change_is_not_overwritten(){
        let mut io=Fake{external:true,..Fake::default()};let r=trial(&mut io,target(),|_|Ok(())).unwrap();
        assert!(!r.success);assert!(r.restore_error.is_some());assert_eq!(io.writes.len(),1);
    }
    #[test]fn persistence_failure_and_power_increase_never_write(){
        let mut io=Fake::default();assert!(trial(&mut io,target(),|_|Err(local("persist","disk full"))).is_err());
        assert!(trial(&mut io,Target{pl1_mw:Some(101000),..Target::default()},|_|panic!("must reject before saving")).is_err());
        assert!(io.writes.is_empty());
    }
    #[test]fn independent_recovery_validates_checkpoint_and_restores(){
        let plan=rapl_plan::prepare(UNITS,ORIGINAL,target()).unwrap();
        let cp=Checkpoint{units_hex:plan.units_hex,original_hex:plan.original_hex,target_hex:plan.target_hex,mask_hex:plan.mask_hex};
        let mut io=Fake::default();io.s.limits=parse(&cp.target_hex).unwrap();
        assert!(recover(&mut io,&cp).unwrap().success);assert_eq!(io.s.limits,ORIGINAL);
        let mut bad=cp;bad.mask_hex="0xFFFFFFFFFFFFFFFF".into();io.writes.clear();
        assert!(recover(&mut io,&bad).is_err());assert!(io.writes.is_empty());
    }
}
