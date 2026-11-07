//! Offline MSR 0x610 edit plans. No driver calls and no proof of platform writability.
//! Only explicitly selected power fields and PL1 time are touched. Enable, clamp,
//! PL2 time, reserved bits and lock are preserved. Dynamic policy is a caller gate.
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, Default)]
pub struct Target { pub pl1_mw:Option<u32>,pub pl2_mw:Option<u32>,pub pl1_time_ms:Option<u64> }
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error { Locked, Disabled, ZeroPower, UnrepresentablePower, UnrepresentableTime, NoChange, ConcurrentChange }
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    pub units_hex:String,pub original_hex:String,pub target_hex:String,pub mask_hex:String,
}
fn power(mw:u32,units:u64)->Result<u64,Error> {
    if mw==0{return Err(Error::ZeroPower);}
    let numerator=u64::from(mw) << (units&15);
    if numerator%1000!=0 || numerator/1000>0x7fff{return Err(Error::UnrepresentablePower);}
    Ok(numerator/1000)
}
fn time(ms:u64,units:u64)->Result<u64,Error> {
    let wanted=u128::from(ms)*4*(1u128<<((units>>16)&15));
    for code in 0..128u64 {
        let represented=1000u128*(1u128<<(code&31))*u128::from(4+(code>>5));
        if wanted==represented{return Ok(code);}
    }
    Err(Error::UnrepresentableTime)
}
pub fn prepare(units:u64,original:u64,target:Target)->Result<Plan,Error> {
    if original&(1<<63)!=0{return Err(Error::Locked);}
    let mut next=original;let mut mask=0u64;
    for (value,shift) in [(target.pl1_mw,0),(target.pl2_mw,32)] {
        if let Some(mw)=value {
            if original&(1u64<<(shift+15))==0{return Err(Error::Disabled);}
            let bits=power(mw,units)?<<shift;let field=0x7fffu64<<shift;
            if next&field!=bits {next=(next&!field)|bits;mask|=field;}
        }
    }
    if let Some(ms)=target.pl1_time_ms {
        if original&(1<<15)==0{return Err(Error::Disabled);}
        let bits=time(ms,units)?<<17;let field=0x7fu64<<17;
        if next&field!=bits {next=(next&!field)|bits;mask|=field;}
    }
    if mask==0{return Err(Error::NoChange);}
    Ok(Plan {units_hex:format!("0x{units:016X}"),original_hex:format!("0x{original:016X}"),target_hex:format!("0x{next:016X}"),mask_hex:format!("0x{mask:016X}")})
}
/// Build a restoration value from a fresh register read, preserving unrelated bits.
/// Only fields still holding our target (or already restored) may be changed back.
/// Each field is considered separately to allow a partial write before failure.
pub fn restore_value(original:u64,target:u64,mask:u64,current:u64)->Result<u64,Error> {
    if current&(1<<63)!=0{return Err(Error::Locked);}
    const FIELDS:[u64;3]=[0x7fff,0x7f<<17,0x7fff<<32];
    let allowed=FIELDS.iter().fold(0,|a,b|a|b);
    if mask==0 || mask&!allowed!=0 || (original^target)&!mask!=0 {return Err(Error::ConcurrentChange);}
    for field in FIELDS {
        let selected=mask&field;
        if selected!=0 && selected!=field{return Err(Error::ConcurrentChange);}
        if selected!=0 && current&field!=target&field && current&field!=original&field {return Err(Error::ConcurrentChange);}
    }
    Ok((current&!mask)|(original&mask))
}
#[cfg(test)] mod tests {
    use super::*;
    const UNITS:u64=0x000A0E03;
    const ORIGINAL:u64=0x004284E800DF8320;
    fn hex(s:&str)->u64 {u64::from_str_radix(s.trim_start_matches("0x"),16).unwrap()}
    #[test]fn actual_snapshot_plan_changes_only_requested_power() {
        let p=prepare(UNITS,ORIGINAL,Target {pl1_mw:Some(75000),..Target::default()}).unwrap();
        let target=hex(&p.target_hex);let mask=hex(&p.mask_hex);
        assert_eq!(target&0x7fff,600);assert_eq!(mask,0x7fff);
        assert_eq!(target&!mask,ORIGINAL&!mask);
        assert_eq!(crate::rapl::decode(Some(UNITS),Some(target)).unwrap().pl1.watts,75.0);
        assert_eq!(restore_value(ORIGINAL,target,mask,target).unwrap(),ORIGINAL);
    }
    #[test]fn exact_time_only_and_unrepresentable_values() {
        let p=prepare(UNITS,ORIGINAL,Target {pl1_time_ms:Some(28000),..Target::default()}).unwrap();
        assert_eq!((hex(&p.target_hex)>>17)&127,0x6e);
        assert_eq!(hex(&p.mask_hex),0x7f<<17);
        assert_eq!(prepare(UNITS,ORIGINAL,Target {pl1_time_ms:Some(27001),..Target::default()}),Err(Error::UnrepresentableTime));
        assert_eq!(prepare(UNITS,ORIGINAL,Target {pl1_mw:Some(75001),..Target::default()}),Err(Error::UnrepresentablePower));
    }
    #[test]fn lock_disabled_overflow_and_noop_are_rejected() {
        let t=Target {pl1_mw:Some(75000),..Target::default()};
        assert_eq!(prepare(UNITS,ORIGINAL|(1<<63),t),Err(Error::Locked));
        assert_eq!(prepare(UNITS,ORIGINAL&!(1<<15),t),Err(Error::Disabled));
        assert_eq!(prepare(UNITS,ORIGINAL,Target {pl1_mw:Some(u32::MAX),..Target::default()}),Err(Error::UnrepresentablePower));
        assert_eq!(prepare(UNITS,ORIGINAL,Target::default()),Err(Error::NoChange));
    }
    #[test]fn restore_preserves_other_changes_and_rejects_owned_field_conflicts() {
        let p=prepare(UNITS,ORIGINAL,Target {pl1_mw:Some(75000),pl1_time_ms:Some(28000),..Target::default()}).unwrap();
        let target=hex(&p.target_hex);let mask=hex(&p.mask_hex);
        let outside=1u64<<25;
        assert_eq!(restore_value(ORIGINAL,target,mask,target^outside).unwrap(),ORIGINAL^outside);
        let partial=(target&!0x7fff)|(ORIGINAL&0x7fff);
        assert_eq!(restore_value(ORIGINAL,target,mask,partial).unwrap(),ORIGINAL);
        assert_eq!(restore_value(ORIGINAL,target,mask,(target&!0x7fff)|500),Err(Error::ConcurrentChange));
        assert_eq!(restore_value(ORIGINAL,target,mask,target|(1<<63)),Err(Error::Locked));
    }
}
