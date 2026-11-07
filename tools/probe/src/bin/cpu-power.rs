//! Standalone explicit IPF probe. No arguments/help never loads a DLL.
use std::path::PathBuf;
use acer_protocol::ipf_power::Limit;

const HELP: &str = "CPU IPF power probe (Windows x64)\n\
  cpu-power --discover --dll ABSOLUTE_IPFIPC_DLL\n\
  cpu-power --read --participant NAME --domain NAME --dll ABSOLUTE_IPFIPC_DLL\n\
  cpu-power --trial --participant NAME --domain NAME --dll ABSOLUTE_IPFIPC_DLL --limit PL1|PL2 --mw INTEGER [--time-ms INTEGER] --receipt NEW_FILE\n\
  cpu-power --restore --participant NAME --domain NAME --dll ABSOLUTE_IPFIPC_DLL --receipt FILE\n\
Read commands use PUBLIC; trial/restore use ELEVATED and change hardware.\n\
Writes require separately arranged validation. PPCC denial aborts before setters.\n\
Discovery is a bounded\n\
snapshot; an empty list is not proof that CPU control is unsupported. --read resolves\n\
names within its own session and rejects duplicates. Handles from earlier processes\n\
are not reused. Trial saves originals durably before writes and restores afterward.\n";
#[derive(Debug, PartialEq, Eq)]
enum Command { Help, Discover { dll: PathBuf }, Read { dll: PathBuf, participant: String, domain: String },
    Trial {dll:PathBuf,participant:String,domain:String,limit:Limit,mw:u32,time_ms:Option<i64>,receipt:PathBuf},
    Restore {dll:PathBuf,participant:String,domain:String,receipt:PathBuf} }
fn parse(args: &[String]) -> Result<Command,String> {
    if args.is_empty() || args == ["--help"] { return Ok(Command::Help); }
    let mode = args[0].as_str();
    if !["--discover","--read","--trial","--restore"].contains(&mode) { return Err("Expected --discover, --read, --trial or --restore".into()); }
    let mut dll=None; let mut participant=None; let mut domain=None;
    let mut limit=None;let mut mw=None;let mut time=None;let mut receipt=None;
    let mut index=1;
    while index<args.len() {
        let value=args.get(index+1).ok_or("Missing option value")?;
        if value.is_empty() || value.starts_with("--") { return Err("Missing option value".into()); }
        let target=match args[index].as_str() {
            "--dll" => &mut dll,
            "--participant" if mode!="--discover" => &mut participant,
            "--domain" if mode!="--discover" => &mut domain,
            "--limit" if mode=="--trial" => &mut limit,
            "--mw" if mode=="--trial" => &mut mw,
            "--time-ms" if mode=="--trial" => &mut time,
            "--receipt" if mode=="--trial" || mode=="--restore" => &mut receipt,
            _ => return Err(format!("Unknown option: {}",args[index])),
        };
        if target.replace(value.clone()).is_some() { return Err("Duplicate option".into()); }
        index+=2;
    }
    let dll=PathBuf::from(dll.ok_or("--dll is required")?);
    if mode=="--discover" {return Ok(Command::Discover {dll});}
    let participant=participant.ok_or("--participant is required")?;let domain=domain.ok_or("--domain is required")?;
    if mode=="--read" {return Ok(Command::Read {dll,participant,domain});}
    let receipt=PathBuf::from(receipt.ok_or("--receipt is required")?);
    if mode=="--restore" {return Ok(Command::Restore {dll,participant,domain,receipt});}
    let limit=match limit.as_deref() {Some("PL1")=>Limit::Pl1,Some("PL2")=>Limit::Pl2,_=>return Err("--limit must be PL1 or PL2".into())};
    let mw=mw.ok_or("--mw is required")?.parse::<u32>().ok().filter(|v|*v>0).ok_or("--mw must be a positive integer")?;
    let time_ms=time.map(|v|v.parse::<i64>().ok().filter(|v|*v>=0).ok_or("--time-ms must be a nonnegative integer")).transpose()?;
    Ok(Command::Trial {dll,participant,domain,limit,mw,time_ms,receipt})
}
#[cfg(all(windows,target_arch="x86_64"))]
#[derive(serde::Serialize,serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    version:u32, machine:acer_device::ipf_session::MachineIdentity,
    participant:String, description:String, domain:String, domain_kind:u32,
    dll_sha256:String, captured_at_ms:u128, checkpoints:Vec<acer_device::cpu_power::Checkpoint>,
}
#[cfg(all(windows,target_arch="x86_64"))]
fn persist(path:&std::path::Path,receipt:&Receipt)->Result<(),acer_device::cpu_power::Error> {
    use std::io::Write;
    let error=|e:String|acer_device::cpu_power::Error {operation:"receipt".into(),code:None,message:e};
    let bytes=serde_json::to_vec_pretty(receipt).map_err(|e|error(e.to_string()))?;
    let mut file=std::fs::OpenOptions::new().write(true).create_new(true).open(path).map_err(|e|error(e.to_string()))?;
    file.write_all(&bytes).and_then(|_|file.sync_all()).map_err(|e|error(e.to_string()))
}
#[cfg(all(windows,target_arch="x86_64"))]
fn run(command: Command) -> Result<(),String> {
    use acer_device::ipf_session::Session;
    use acer_protocol::ipf_power::{Access,Limit,Operation,Value};
    use std::time::{Duration,Instant};
    enum Write { Trial(Limit,u32,Option<i64>,PathBuf), Restore(PathBuf) }
    let (dll,selection,write)=match command {
        Command::Help => { print!("{HELP}"); return Ok(()); }
        Command::Discover {dll} => (dll,None,None),
        Command::Read {dll,participant,domain} => (dll,Some((participant,domain)),None),
        Command::Trial {dll,participant,domain,limit,mw,time_ms,receipt} => (dll,Some((participant,domain)),Some(Write::Trial(limit,mw,time_ms,receipt))),
        Command::Restore {dll,participant,domain,receipt} => (dll,Some((participant,domain)),Some(Write::Restore(receipt))),
    };
    let recovery=if let Some(Write::Restore(path))=&write {
        let bytes=std::fs::read(path).map_err(|e|e.to_string())?;
        Some(serde_json::from_slice::<Receipt>(&bytes).map_err(|e|e.to_string())?)
    }else{None};
    let binding=if write.is_some() {
        use sha2::{Digest,Sha256};
        let machine=acer_device::ipf_session::machine_identity().map_err(|e|format!("{e:?}"))?;
        let hash=format!("{:x}",Sha256::digest(std::fs::read(&dll).map_err(|e|e.to_string())?));
        if let Some(saved)=&recovery {
            if saved.version!=1 || saved.machine!=machine || saved.dll_sha256!=hash {return Err("Recovery receipt machine, BIOS or DLL identity mismatch".into());}
        }
        Some((machine,hash))
    }else{None};
    let access=if write.is_some(){Access::SupervisedWrite}else{Access::ReadOnly};
    let mut session=Session::connect(&dll,access).map_err(|e|format!("{e:?}"))?;
    // Allow asynchronous AppCreate/participant/domain callbacks to arrive. This is
    // a snapshot window, not a claim that all possible devices were enumerated.
    let until=Instant::now()+Duration::from_secs(2);
    while Instant::now()<until { std::thread::sleep(Duration::from_millis(20)); }
    let participants=session.participants().map_err(|e|format!("{e:?}"))?;
    let Some((pname,dname))=selection else {
        println!("{}",serde_json::to_string_pretty(&serde_json::json!({"endpoint":"public","snapshot_only":true,"participants":participants})).map_err(|e|e.to_string())?);
        return Ok(());
    };
    let candidates:Vec<_>=participants.iter().filter(|p|p.name==pname)
        .flat_map(|p|p.domains.values().filter(|d|d.name==dname).map(move |d|(p,d))).collect();
    if candidates.len()!=1 { return Err(format!("Expected one participant/domain named {pname}/{dname}; found {}",candidates.len())); }
    let (participant,domain)=candidates[0];
    if let Some(write)=write {
        let (machine,hash)=binding.expect("write identity established");
        let (report,success)=match write {
            Write::Trial(limit,mw,time_ms,path)=> {
                let changes=[acer_device::cpu_power::Change {limit,power_mw:mw,time_ms}];
                let report=session.trial(participant.handle,domain.handle,&changes,|points|persist(&path,&Receipt {
                    version:1,machine:machine.clone(),participant:participant.name.clone(),description:participant.description.clone(),
                    domain:domain.name.clone(),domain_kind:domain.kind,dll_sha256:hash.clone(),
                    captured_at_ms:std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|e|acer_device::cpu_power::Error {operation:"clock".into(),code:None,message:e.to_string()})?.as_millis(),
                    checkpoints:points.to_vec(),
                })).map_err(|e|format!("{e:?}"))?;
                let success=report.success();(serde_json::json!({"receipt":path,"trial":report}),success)
            }
            Write::Restore(path)=> {
                let saved=recovery.expect("receipt parsed before connection");
                if saved.participant!=participant.name || saved.description!=participant.description || saved.domain!=domain.name || saved.domain_kind!=domain.kind {
                    return Err("Recovery participant/domain identity mismatch".into());
                }
                let report=session.recover(participant.handle,domain.handle,&saved.checkpoints).map_err(|e|format!("{e:?}"))?;
                let success=report.success();(serde_json::json!({"receipt":path,"recovery":report}),success)
            }
        };
        println!("{}",serde_json::to_string_pretty(&serde_json::json!({"endpoint":"elevated","success":success,"report":report})).map_err(|e|e.to_string())?);
        return if success {Ok(())}else{Err("Write/readback/recovery did not fully succeed; retain the receipt".into())};
    }
    fn value(v:Value)->serde_json::Value { match v {
        Value::LimitMw(v)=>serde_json::json!({"milliwatts":v}), Value::Enabled(v)=>serde_json::json!({"enabled":v}),
        Value::TimeWindowMs(v)=>serde_json::json!({"milliseconds":v}), Value::Acknowledged=>serde_json::json!({"acknowledged":true}),
    } }
    let mut results=Vec::new(); let mut failed=false;
    for limit in [Limit::Pl1,Limit::Pl2] {
        for operation in [Operation::GetLimit,Operation::GetEnabled,Operation::GetTimeWindow] {
            let result=match session.read(participant.handle,domain.handle,limit,operation) {
                Ok(v)=>serde_json::json!({"value":value(v)}),
                Err(e)=>{ failed=true;serde_json::json!({"error":e}) },
            };
            results.push(serde_json::json!({"limit":limit,"operation":operation,"result":result}));
        }
    }
    let capabilities=match session.capabilities(participant.handle,domain.handle) {
        Ok(rows)=>serde_json::json!({"rows":rows}),
        Err(e)=>serde_json::json!({"error":e}),
    };
    println!("{}",serde_json::to_string_pretty(&serde_json::json!({"endpoint":"public","participant":participant,"domain":domain,"readbacks":results,"capabilities":capabilities})).map_err(|e|e.to_string())?);
    if failed { Err("One or more primitive reads failed; see structured errors above".into()) } else { Ok(()) }
}
#[cfg(not(all(windows,target_arch="x86_64")))]
fn run(command:Command)->Result<(),String> { if command==Command::Help { print!("{HELP}");Ok(()) } else { Err("Windows x64 is required".into()) } }
fn main() {
    let result=parse(&std::env::args().skip(1).collect::<Vec<_>>()).and_then(run);
    if let Err(e)=result { eprintln!("{e}");std::process::exit(1); }
}
#[cfg(test)] mod tests {
    use super::*;
    fn args(s:&[&str])->Vec<String>{s.iter().map(|v|v.to_string()).collect()}
    #[test] fn no_implicit_connection_or_incomplete_write_commands() {
        assert_eq!(parse(&[]).unwrap(),Command::Help);
        assert_eq!(parse(&args(&["--help"])).unwrap(),Command::Help);
        for a in [vec!["--discover"],vec!["--trial"],vec!["--read","--dll","x","--sdk","test"],vec!["--discover","--dll","x","--sdk","test","--sdk","test"]] {
            assert!(parse(&args(&a)).is_err());
        }
        assert!(matches!(parse(&args(&["--read","--dll","x","--participant","CPU","--domain","D0"])).unwrap(),Command::Read{..}));
    }
    #[test] fn trial_requires_integer_target_and_recovery_path() {
        let base=["--trial","--dll","x","--participant","CPU","--domain","D0","--limit","PL1","--mw","40000","--receipt","new.json"];
        assert!(matches!(parse(&args(&base)).unwrap(),Command::Trial{mw:40000,limit:Limit::Pl1,..}));
        for bad in ["0","-1","40.5","4294967296"] {
            let mut a=args(&base);a[10]=bad.into();assert!(parse(&a).is_err());
        }
        let mut a=args(&base);a.extend(args(&["--time-ms","-1"]));assert!(parse(&a).is_err());
        assert!(parse(&args(&base[..11])).is_err());
        assert!(parse(&args(&["--restore","--dll","x","--participant","CPU","--domain","D0"])).is_err());
    }
    #[cfg(all(windows,target_arch="x86_64"))]
    #[test] fn durable_receipt_roundtrip_and_no_overwrite() {
        let path=std::env::temp_dir().join(format!("cpu-receipt-{}-{}.json",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let receipt=Receipt {version:1,machine:acer_device::ipf_session::MachineIdentity {uuid:"test-uuid".into(),model:"test-model".into(),bios:"test-bios".into()},participant:"CPU".into(),description:"test".into(),domain:"D0".into(),domain_kind:0,dll_sha256:"test-hash".into(),captured_at_ms:1,
            checkpoints:vec![acer_device::cpu_power::Checkpoint {limit:Limit::Pl1,original:acer_device::cpu_power::State {power_mw:45000,time_ms:Some(28000),enabled:true},target:acer_device::cpu_power::State {power_mw:40000,time_ms:Some(28000),enabled:true},capability:acer_device::cpu_power::Capability {min_mw:10000,max_mw:55000,time_bounds_ms:Some((0,28000))}}]};
        persist(&path,&receipt).unwrap();let bytes=std::fs::read(&path).unwrap();
        let decoded:Receipt=serde_json::from_slice(&bytes).unwrap();assert_eq!(decoded.machine,receipt.machine);assert_eq!(decoded.checkpoints[0].original.power_mw,45000);
        assert!(persist(&path,&receipt).is_err());assert_eq!(std::fs::read(&path).unwrap(),bytes);
        assert!(serde_json::from_slice::<Receipt>(&bytes[..bytes.len()-1]).is_err());
        std::fs::remove_file(path).unwrap();
    }
}
