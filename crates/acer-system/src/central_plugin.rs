//! Management of the separately installed, fixed Acer Handle Guard task.
use super::{error, SystemError};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::{Read, Seek, SeekFrom};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GuardAction { Enable, Disable }

const QUERY: &str = include_str!("central_plugin.ps1");

#[cfg(windows)]
fn run(action: Option<GuardAction>) -> Result<Value, SystemError> {
    let action = match action { None=>"query", Some(GuardAction::Enable)=>"enable", Some(GuardAction::Disable)=>"disable" };
    run_fixed_script(&format!("$guardAction='{action}'\n{QUERY}"))
}
/// Internal native callers supply embedded scripts only, never renderer commands.
#[cfg(windows)]
pub fn run_fixed_script(script: &str) -> Result<Value, SystemError> {
    use std::os::windows::{ffi::OsStringExt, process::CommandExt};
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};
    #[link(name="kernel32")] unsafe extern "system" { fn GetSystemDirectoryW(buffer:*mut u16,size:u32)->u32; }
    let mut path = vec![0u16; 32768];
    let n = unsafe { GetSystemDirectoryW(path.as_mut_ptr(), path.len() as u32) } as usize;
    if n == 0 || n >= path.len() { return Err(error("Cannot locate Windows system directory")); }
    let exe = std::path::PathBuf::from(std::ffi::OsString::from_wide(&path[..n])).join("WindowsPowerShell/v1.0/powershell.exe");
    let mut child = Command::new(exe).args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command", script])
        .creation_flags(0x08000000).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(error)?;
    let out = child.stdout.take().ok_or_else(||error("Missing stdout"))?;
    let err = child.stderr.take().ok_or_else(||error("Missing stderr"))?;
    // Drain concurrently so a pipe buffer cannot stall the child before timeout.
    fn drain(mut input: impl Read) -> std::io::Result<Vec<u8>> {
        let mut kept=Vec::new(); let mut buf=[0u8;4096];
        loop { let n=input.read(&mut buf)?; if n==0 {break} if kept.len()+n<=262144 {kept.extend_from_slice(&buf[..n]);} }
        Ok(kept)
    }
    let stdout=std::thread::spawn(move||drain(out));
    let stderr=std::thread::spawn(move||drain(err));
    let start=Instant::now();
    let status=loop {
        match child.try_wait() {
            Ok(Some(status))=>break Ok(status),
            Ok(None) if start.elapsed()<Duration::from_secs(10)=>std::thread::sleep(Duration::from_millis(30)),
            result=>{let _=child.kill();let _=child.wait();break Err(error(match result {Err(e)=>e.to_string(),_=>"Plugin query timed out; refresh to confirm task state".into()}));}
        }
    };
    let out=stdout.join().map_err(|_|error("Plugin output thread failed"))?.map_err(error)?;
    let err=stderr.join().map_err(|_|error("Plugin error thread failed"))?.map_err(error)?;
    if !status?.success() {return Err(error(String::from_utf8_lossy(&err)));}
    serde_json::from_slice(out.strip_prefix(&[0xef,0xbb,0xbf]).unwrap_or(&out)).map_err(error)
}
#[cfg(not(windows))]
fn run(_:Option<GuardAction>)->Result<Value,SystemError>{Err(error("Plugin requires Windows"))}
#[cfg(not(windows))]
pub fn run_fixed_script(_: &str)->Result<Value,SystemError>{Err(error("Plugin requires Windows"))}
pub fn query()->Result<Value,SystemError>{run(None)}
pub fn set_guard(action:GuardAction)->Result<Value,SystemError>{run(Some(action))}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all="snake_case")]
pub enum PluginLog { Events, PreviousEvents, LastCheck, Activation }
impl PluginLog {
    fn relative(self)-> &'static str {match self {
        Self::Events=>"AcerHandleGuard/events.log",Self::PreviousEvents=>"AcerHandleGuard/events.previous.log",
        Self::LastCheck=>"AcerHandleGuard/last-check.txt",Self::Activation=>"AcerCentralPatch/activation-result.json"
    }}
}
fn tail(path:&std::path::Path)->Result<Value,SystemError>{
    const LIMIT:u64=32*1024;
    let mut file=std::fs::File::open(path).map_err(error)?;
    let size=file.metadata().map_err(error)?.len();
    let offset=size.saturating_sub(LIMIT);file.seek(SeekFrom::Start(offset)).map_err(error)?;
    let mut bytes=Vec::new();file.take(LIMIT).read_to_end(&mut bytes).map_err(error)?;
    // Drop a partial first line when returning only the end of a log.
    if offset>0 {if let Some(n)=bytes.iter().position(|b|*b==b'\n'){bytes.drain(..=n);}}
    Ok(serde_json::json!({"text":String::from_utf8_lossy(&bytes).trim_start_matches('\u{feff}'),"truncated":offset>0,"file_bytes":size}))
}
pub fn read_log(log:PluginLog)->Result<Value,SystemError>{
    #[cfg(windows)] {
        // Resolve the machine ProgramData folder, not a renderer-provided path.
        #[link(name="shell32")] unsafe extern "system" {fn SHGetFolderPathW(hwnd:*mut std::ffi::c_void,csidl:i32,token:*mut std::ffi::c_void,flags:u32,path:*mut u16)->i32;}
        let mut buf=[0u16;260];
        if unsafe{SHGetFolderPathW(std::ptr::null_mut(),0x23,std::ptr::null_mut(),0,buf.as_mut_ptr())}<0{return Err(error("ProgramData unavailable"));}
        let n=buf.iter().position(|x|*x==0).unwrap_or(buf.len());
        tail(&std::path::PathBuf::from(String::from_utf16_lossy(&buf[..n])).join(log.relative()))
    }
    #[cfg(not(windows))] {let _=log;Err(error("Plugin logs require Windows"))}
}

#[cfg(test)]mod tests {
    use super::*;
    #[test]fn closed_actions_and_logs(){
        assert!(serde_json::from_str::<GuardAction>("\"restart\"").is_err());
        assert!(serde_json::from_str::<PluginLog>("\"../../secrets\"").is_err());
        assert_eq!(PluginLog::Events.relative(),"AcerHandleGuard/events.log");
    }
    #[test]fn log_tail_is_bounded(){
        let path=std::env::temp_dir().join(format!("acer-plugin-tail-{}.txt",std::process::id()));
        let data="line\n".repeat(10000);std::fs::write(&path,data).unwrap();
        let result=tail(&path).unwrap();std::fs::remove_file(path).unwrap();
        assert_eq!(result["truncated"],true);assert!(result["text"].as_str().unwrap().len()<=32768);
        assert!(result["text"].as_str().unwrap().ends_with("line\n"));
    }
}
