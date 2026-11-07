use super::*;
use sha2::{Digest, Sha256};
const TASK: &str = r"\PredatorSenseLauncher";
const NS: &str = "http://schemas.microsoft.com/windows/2004/02/mit/task";
const MAX_XML: usize = 1024 * 1024;
static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OfficialTaskState { pub task_name: String, pub enabled: bool, pub definition_sha256: String }
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest { version: u32, task_name: String, original_enabled: bool, definition_sha256: String }
fn parse(xml: &str) -> Result<OfficialTaskState, SystemError> {
    if xml.len() > MAX_XML { return Err(error("Task XML exceeds limit")); }
    let doc = roxmltree::Document::parse(xml).map_err(error)?;
    let root = doc.root_element();
    if !root.has_tag_name((NS,"Task")) { return Err(error("Unexpected task XML root")); }
    let settings = root.children().find(|n|n.has_tag_name((NS,"Settings")));
    let enabled_node = settings.and_then(|n|n.children().find(|n|n.has_tag_name((NS,"Enabled"))));
    let enabled = match enabled_node.and_then(|n|n.text()).map(str::trim) { None | Some("true") | Some("1") => true, Some("false") | Some("0") => false, _ => return Err(error("Invalid task Enabled field")) };
    fn field(hash: &mut Sha256, s: &str) { hash.update((s.len() as u64).to_le_bytes()); hash.update(s.as_bytes()); }
    fn visit(node: roxmltree::Node<'_, '_>, skip: Option<roxmltree::NodeId>, hash: &mut Sha256) {
        if Some(node.id()) == skip { return; }
        if node.is_element() {
            hash.update([1]); field(hash,node.tag_name().namespace().unwrap_or("")); field(hash,node.tag_name().name());
            let mut attrs:Vec<_> = node.attributes().collect();
            attrs.sort_by_key(|a|(a.namespace().unwrap_or(""),a.name()));
            for a in attrs { hash.update([2]); field(hash,a.namespace().unwrap_or("")); field(hash,a.name()); field(hash,a.value()); }
            for child in node.children() { visit(child,skip,hash); }
            hash.update([3]);
        } else if node.is_text() {
            let text=node.text().unwrap_or("");
            let indentation=text.trim().is_empty() && node.parent().is_some_and(|p|p.children().any(|n|n.is_element()));
            if !indentation { hash.update([4]); field(hash,text); }
        }
    }
    let mut hash=Sha256::new(); visit(root,enabled_node.map(|n|n.id()),&mut hash);
    Ok(OfficialTaskState { task_name:TASK.into(), enabled, definition_sha256:format!("{:x}",hash.finalize()) })
}
#[cfg(windows)] fn run(args: &[&str]) -> Result<Vec<u8>, SystemError> {
    use std::os::windows::{ffi::OsStringExt, process::CommandExt};
    #[link(name="kernel32")] unsafe extern "system" { fn GetSystemDirectoryW(buffer:*mut u16,size:u32)->u32; }
    let mut buffer=vec![0u16;32768];
    let size=unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(),buffer.len() as u32) } as usize;
    if size==0 || size>=buffer.len() { return Err(error("Cannot locate Windows system directory")); }
    let executable=std::path::PathBuf::from(std::ffi::OsString::from_wide(&buffer[..size])).join("schtasks.exe");
    let output=std::process::Command::new(executable).args(args).creation_flags(0x08000000).output().map_err(error)?;
    if !output.status.success() { return Err(error(format!("Fixed official task command failed ({}): {}. Task mutation may require an administrator terminal; no automatic elevation was requested.",output.status,String::from_utf8_lossy(&output.stderr)))); }
    if output.stdout.len()>MAX_XML { return Err(error("Task command output exceeds limit")); }
    Ok(output.stdout)
}
#[cfg(not(windows))] fn run(_: &[&str]) -> Result<Vec<u8>, SystemError> { Err(error("Official task integration requires Windows")) }
fn decode(bytes: &[u8]) -> Result<String, SystemError> {
    if bytes.starts_with(&[0xff,0xfe]) || bytes.get(1)==Some(&0) {
        let bytes=bytes.strip_prefix(&[0xff,0xfe]).unwrap_or(bytes);
        if bytes.len()%2!=0 { return Err(error("Invalid UTF-16 task XML")); }
        let words:Vec<u16>=bytes.chunks_exact(2).map(|c|u16::from_le_bytes([c[0],c[1]])).collect();
        String::from_utf16(&words).map_err(error)
    } else { String::from_utf8(bytes.strip_prefix(&[0xef,0xbb,0xbf]).unwrap_or(bytes).to_vec()).map_err(error) }
}
fn query() -> Result<(String,OfficialTaskState),SystemError> {
    let xml=decode(&run(&["/Query","/TN",TASK,"/XML"])?)?;
    let state=parse(&xml)?; Ok((xml,state))
}
/// Read-only; no task creation and no automatic elevation.
pub fn get_official_task() -> Result<OfficialTaskState,SystemError> { Ok(query()?.1) }
#[derive(Debug,Serialize)]
pub struct OfficialBackupState{pub backup_valid:bool,pub backup_original_enabled:Option<bool>,pub backup_error:Option<String>}
pub fn get_official_backup(config_dir:&Path)->OfficialBackupState{
    if !directory(config_dir).exists(){return OfficialBackupState{backup_valid:false,backup_original_enabled:None,backup_error:None}}
    match validated_backup(config_dir){Ok(m)=>OfficialBackupState{backup_valid:true,backup_original_enabled:Some(m.original_enabled),backup_error:None},Err(e)=>OfficialBackupState{backup_valid:false,backup_original_enabled:None,backup_error:Some(e.to_string())}}
}
fn directory(config_dir:&Path)->std::path::PathBuf { config_dir.join("official-task-backup") }
/// Explicit file backup only. Never overwrites an existing backup or changes task state.
pub fn backup_official_task(config_dir:&Path)->Result<OfficialTaskState,SystemError> {
    use std::io::Write;
    let _guard=LOCK.lock().map_err(error)?;
    let (xml,state)=query()?;
    std::fs::create_dir_all(config_dir).map_err(error)?;
    let dir=directory(config_dir); std::fs::create_dir(&dir).map_err(error)?;
    let manifest=Manifest { version:1,task_name:TASK.into(),original_enabled:state.enabled,definition_sha256:state.definition_sha256.clone() };
    for (name,bytes) in [("task.xml",xml.into_bytes()),("manifest.json",serde_json::to_vec_pretty(&manifest).map_err(error)?)] {
        let mut file=std::fs::OpenOptions::new().write(true).create_new(true).open(dir.join(name)).map_err(error)?;
        file.write_all(&bytes).map_err(error)?; file.sync_all().map_err(error)?;
    }
    Ok(state)
}
fn validated_backup(config_dir:&Path)->Result<Manifest,SystemError> {
    let dir=directory(config_dir);
    let manifest:Manifest=serde_json::from_slice(&std::fs::read(dir.join("manifest.json")).map_err(error)?).map_err(error)?;
    let xml=std::fs::read_to_string(dir.join("task.xml")).map_err(error)?;
    let saved=parse(&xml)?;
    if manifest.version!=1 || manifest.task_name!=TASK || manifest.definition_sha256!=saved.definition_sha256 || manifest.original_enabled!=saved.enabled { return Err(error("Official task backup identity or digest mismatch")); }
    let live=query()?.1;
    if live.definition_sha256!=saved.definition_sha256 { return Err(error("Official task definition changed; review saved XML before changing task")); }
    Ok(manifest)
}
fn change(enabled:bool)->Result<OfficialTaskState,SystemError> {
    run(&["/Change","/TN",TASK,if enabled {"/ENABLE"} else {"/DISABLE"}])?;
    let state=query()?.1;
    if state.enabled!=enabled { return Err(error("Official task readback mismatch; inspect current task state")); }
    Ok(state)
}
/// Explicit user action; a previously saved, matching backup is required.
pub fn disable_official_task(config_dir:&Path)->Result<OfficialTaskState,SystemError> {
    let _guard=LOCK.lock().map_err(error)?;
    validated_backup(config_dir)?; change(false)
}
/// Restores only the backup's original Enabled state, never recreates task definitions.
pub fn restore_official_task(config_dir:&Path)->Result<OfficialTaskState,SystemError> {
    let _guard=LOCK.lock().map_err(error)?;
    let manifest=validated_backup(config_dir)?; change(manifest.original_enabled)
}
#[cfg(test)] mod tests {
    use super::*;
    fn xml(enabled:bool)->String { format!(r#"<Task xmlns="{NS}"><Settings><Enabled>{enabled}</Enabled></Settings><Actions><Exec><Command>C:\fixed.exe</Command></Exec></Actions></Task>"#) }
    #[test] fn digest_ignores_only_task_enabled() {
        let a=parse(&xml(true)).unwrap();let b=parse(&xml(false)).unwrap();
        assert!(a.enabled);assert!(!b.enabled);assert_eq!(a.definition_sha256,b.definition_sha256);
        assert_ne!(a.definition_sha256,parse(&xml(true).replace("fixed.exe","other.exe")).unwrap().definition_sha256);
    }
    #[test] fn decode_utf16_and_reject_wrong_root() {
        let text=xml(true);let bytes:Vec<u8>=std::iter::once(0xfeffu16).chain(text.encode_utf16()).flat_map(u16::to_le_bytes).collect();
        assert_eq!(decode(&bytes).unwrap(),text);assert!(parse("<not-a-task/>").is_err());
    }
}
