//! Explicit MSR-only validation entry. Help never opens a driver.
use std::path::{Path,PathBuf};
use acer_device::{cpu_power::Error,msr_power::Checkpoint};
use serde::{Serialize,Deserialize};
const HELP:&str="MSR power probe, reviewed PHN16-71 / BIOS V1.18 only\n\
  msr-power --read MODULE\n\
  msr-power --trial MODULE PL1|PL2 MILLIWATTS --receipt NEW_FILE [--time-ms MILLISECONDS]\n\
  msr-power --restore MODULE RECEIPT_FILE\n\
Trial lowers an enabled MSR limit, saves originals, reads back, then restores.\n\
Time edits are PL1-only. Changes do not imply effective MMIO/GPU power control.\n\
Trial/restore write hardware and require separately arranged validation.\n\
No elevation, driver installation, service startup, EFI or UI integration.\n";
#[derive(Debug,PartialEq,Eq)]
enum Mode{Help,Read(PathBuf),Trial{module:PathBuf,pl:u8,mw:u32,time_ms:Option<u64>,receipt:PathBuf},Restore(PathBuf,PathBuf)}
fn parse(a:&[String])->Result<Mode,String>{
    if a.is_empty() || a==["--help"]{return Ok(Mode::Help);}
    match a {
        [mode,module] if mode=="--read"=>Ok(Mode::Read(module.into())),
        [mode,module,file] if mode=="--restore"=>Ok(Mode::Restore(module.into(),file.into())),
        [mode,module,limit,mw,flag,file,tail @ ..] if mode=="--trial" && flag=="--receipt"=>{
            let pl=match limit.as_str(){"PL1"=>1,"PL2"=>2,_=>return Err("Limit must be PL1 or PL2".into())};
            let mw=mw.parse::<u32>().ok().filter(|v|*v>0).ok_or("Positive integer milliwatts required")?;
            let time_ms=match tail{[]=>None,[flag,ms] if flag=="--time-ms" && pl==1=>Some(ms.parse::<u64>().ok().filter(|v|*v>0).ok_or("Positive integer milliseconds required")?),_=>return Err("Unexpected options or unsupported PL2 time edit".into())};
            Ok(Mode::Trial{module:module.into(),pl,mw,time_ms,receipt:file.into()})
        },
        _=>Err("Invalid command; use --help".into()),
    }
}
#[derive(Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt{version:u32,machine_uuid:String,model:String,bios:String,captured_at_ms:u128,checkpoint:Checkpoint}
fn error(op:&str,e:impl ToString)->Error{Error{operation:op.into(),code:None,message:e.to_string()}}
fn persist(path:&Path,r:&Receipt)->Result<(),Error>{
    use std::io::Write;
    let bytes=serde_json::to_vec_pretty(r).map_err(|e|error("receipt",e))?;
    let mut file=std::fs::OpenOptions::new().write(true).create_new(true).open(path).map_err(|e|error("receipt",e))?;
    file.write_all(&bytes).and_then(|_|file.sync_all()).map_err(|e|error("receipt",e))
}
#[cfg(all(windows,target_arch="x86_64"))]
fn run(mode:Mode)->Result<(),Error>{
    use acer_device::{ipf_session::machine_identity,pawn_msr::PawnMsr};
    use acer_protocol::rapl_plan::Target;
    if mode==Mode::Help{print!("{HELP}");return Ok(());}
    let identity=machine_identity()?;
    if !matches!(identity.model.as_str(),"PHN16-71"|"Predator PHN16-71") || identity.bios!="V1.18" {
        return Err(error("identity","This entry is bound to reviewed PHN16-71 / V1.18"));
    }
    let (value,success)=match mode {
        Mode::Help=>unreachable!(),
        Mode::Read(module)=>{
            let mut io=PawnMsr::open(&module,false)?;let s=io.snapshot()?;
            (serde_json::json!({"identity":identity,"source":"MSR 0x606/0x610","decoded":acer_protocol::rapl::decode(Some(s.units),Some(s.limits))}),true)
        },
        Mode::Trial{module,pl,mw,time_ms,receipt}=>{
            let mut io=PawnMsr::open(&module,true)?;
            let target=Target{pl1_mw:if pl==1{Some(mw)}else{None},pl2_mw:if pl==2{Some(mw)}else{None},pl1_time_ms:time_ms};
            let report=io.trial(target,|cp|persist(&receipt,&Receipt{version:1,machine_uuid:identity.uuid.clone(),model:identity.model.clone(),bios:identity.bios.clone(),
                captured_at_ms:std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|e|error("clock",e))?.as_millis(),checkpoint:cp.clone()}))?;
            let success=report.success;(serde_json::json!({"receipt":receipt,"report":report}),success)
        },
        Mode::Restore(module,path)=>{
            let bytes=std::fs::read(&path).map_err(|e|error("receipt",e))?;
            let r:Receipt=serde_json::from_slice(&bytes).map_err(|e|error("receipt",e))?;
            if r.version!=1 || r.machine_uuid!=identity.uuid || r.model!=identity.model || r.bios!=identity.bios{return Err(error("identity","Receipt machine/BIOS mismatch"));}
            let mut io=PawnMsr::open(&module,true)?;let report=io.recover(&r.checkpoint)?;
            let success=report.success;(serde_json::json!({"receipt":path,"recovery":report}),success)
        },
    };
    println!("{}",serde_json::to_string_pretty(&value).map_err(|e|error("json",e))?);
    if success{Ok(())}else{Err(error("validation","Write/readback/recovery not fully successful; keep receipt"))}
}
#[cfg(not(all(windows,target_arch="x86_64")))]
fn run(mode:Mode)->Result<(),Error>{if mode==Mode::Help{print!("{HELP}");Ok(())}else{Err(error("platform","Windows x64 required"))}}
fn main(){let result=parse(&std::env::args().skip(1).collect::<Vec<_>>()).map_err(|e|error("arguments",e)).and_then(run);if let Err(e)=result{eprintln!("{}",serde_json::to_string_pretty(&e).unwrap());std::process::exit(1);}}
#[cfg(test)]mod tests{
    use super::*;
    fn args(s:&str)->Vec<String>{s.split_whitespace().map(str::to_owned).collect()}
    #[test]fn commands_are_explicit_and_typed(){
        assert_eq!(parse(&[]).unwrap(),Mode::Help);
        for s in ["--trial m PL1 75000","--trial m PL1 0 --receipt r","--trial m PL1 75.5 --receipt r","--trial m PL2 75000 --receipt r --time-ms 28000","--restore m"]{assert!(parse(&args(s)).is_err());}
        assert!(matches!(parse(&args("--trial m PL1 75000 --receipt r --time-ms 28000")).unwrap(),Mode::Trial{pl:1,time_ms:Some(28000),..}));
    }
    #[test]fn receipt_persists_original_without_overwriting(){
        let path=std::env::temp_dir().join(format!("msr-receipt-{}-{}.json",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let r=Receipt{version:1,machine_uuid:"test".into(),model:"test".into(),bios:"test".into(),captured_at_ms:1,checkpoint:Checkpoint{units_hex:"0x00000000000A0E03".into(),original_hex:"0x004284E800DF8320".into(),target_hex:"0x004284E800DF8258".into(),mask_hex:"0x0000000000007FFF".into()}};
        persist(&path,&r).unwrap();let bytes=std::fs::read(&path).unwrap();assert!(persist(&path,&r).is_err());assert_eq!(bytes,std::fs::read(&path).unwrap());
        let decoded:Receipt=serde_json::from_slice(&bytes).unwrap();assert_eq!(decoded.checkpoint.original_hex,r.checkpoint.original_hex);std::fs::remove_file(path).unwrap();
    }
}
