//! Native-only fixed-target transactions. WebView callers must use a helper-held
//! plan handle; a serialized Discovery or caller-supplied destination grants no authority.
use super::*;
use locked_directory::LockedDirectory;
use native_discovery::Discovery;

#[derive(Debug,Serialize,Deserialize,PartialEq,Eq)]
#[serde(deny_unknown_fields)]
struct Binding { schema:u32, model:String, bios:String, esp_volume:String, cache_volume_path:PathBuf }
#[derive(Debug,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal { schema:u32, state:String, desired:Manifest }
#[derive(Debug,Serialize)]
pub struct NativeReport { pub backup_id:String, pub transaction:Report, pub journal_error:Option<String> }
#[derive(Debug,Serialize)]
pub struct BackupSummary { pub id:String, pub state:String }
#[derive(Debug,Serialize)]
pub struct TargetInspection { pub discovery:Discovery, pub states:Manifest }
pub struct NativePlan {
    plan:Plan,
    backup_id:String,
    _guards:Vec<LockedDirectory>,
}
enum Action<'a> { Image(&'a Path), Factory, Restore(&'a str) }

fn discovered()->io::Result<Discovery> { native_discovery::discover().map_err(|e|invalid(&e)) }
/// Pins the existing real directories for inspection without creating anything.
pub fn inspect_current()->io::Result<TargetInspection> {
    let discovery=discovered()?;
    let esp=LockedDirectory::open(&discovery.esp_oem)?;
    let cache=LockedDirectory::open(&discovery.cache)?;
    let states=Targets::inspect(esp.path(),cache.path())?.states()?;
    Ok(TargetInspection{discovery,states})
}
fn backup_root(discovery:&Discovery)->io::Result<PathBuf> {
    // discovery.cache is constructed internally from KnownFolder ProgramData.
    let program_data=discovery.cache.ancestors().nth(3).ok_or_else(||invalid("missing ProgramData parent"))?;
    Ok(program_data.join("AcerConsole").join("BootLogoBackups"))
}
fn valid_id(id:&str)->bool { !id.is_empty()&&id.len()<=80&&id.bytes().all(|b|b.is_ascii_digit()||b==b'-') }
fn create_locked(path:&Path)->io::Result<LockedDirectory> {
    match LockedDirectory::open(path) {
        Ok(guard)=>Ok(guard),
        Err(e) if e.kind()==io::ErrorKind::NotFound=>{
            let parent=path.parent().ok_or_else(||invalid("missing directory root"))?;
            let _parent=create_locked(parent)?;
            match fs::create_dir(path) { Ok(())=>(),Err(e) if e.kind()==io::ErrorKind::AlreadyExists=>(),Err(e)=>return Err(e) }
            LockedDirectory::open(path)
        },
        Err(e)=>Err(e),
    }
}
fn binding(discovery:&Discovery,cache:&LockedDirectory)->Binding {
    #[cfg(windows)] let cache_volume_path=cache.canonical_guid_path().to_path_buf();
    #[cfg(not(windows))] let cache_volume_path=cache.path().to_path_buf();
    Binding{schema:1,model:discovery.model.clone(),bios:discovery.bios.clone(),esp_volume:discovery.esp_volume.clone(),cache_volume_path}
}
fn next_id()->String {
    static SEQUENCE:std::sync::atomic::AtomicU64=std::sync::atomic::AtomicU64::new(0);
    format!("{}-{}-{}",std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos(),std::process::id(),SEQUENCE.fetch_add(1,std::sync::atomic::Ordering::Relaxed))
}
fn write_journal(directory:&Path,state:&str,desired:Manifest)->io::Result<()> {
    let temporary=directory.join("transaction.pending");
    write_new_synced(&temporary,&serde_json::to_vec_pretty(&Journal{schema:1,state:state.into(),desired})?)?;
    atomic_replace(&temporary,&directory.join("transaction.json"))
}
fn prepare(discovery:Discovery,action:Action<'_>)->io::Result<NativePlan> {
    let esp=create_locked(&discovery.esp_oem)?;
    let cache=create_locked(&discovery.cache)?;
    let root=create_locked(&backup_root(&discovery)?)?;
    let current_binding=binding(&discovery,&cache);
    // Locks remain held across preparation, UI review, commit, readback and rollback.
    let targets=Targets::inspect(esp.path(),cache.path())?;
    let id=next_id();let backup=root.path().join(&id);
    let mut guards=vec![esp,cache,root];
    let plan=match action {
        Action::Image(path)=>targets.prepare(Some(path),(discovery.display_pixels[0],discovery.display_pixels[1]),&backup)?,
        Action::Factory=>targets.prepare(None,(discovery.display_pixels[0],discovery.display_pixels[1]),&backup)?,
        Action::Restore(id)=>{
            if !valid_id(id){return Err(invalid("invalid backup ID"))}
            let source=LockedDirectory::open(&guards[2].path().join(id))?;
            let bytes=read(&source.path().join("binding.json"))?.ok_or_else(||invalid("backup binding is missing"))?;
            let saved:Binding=serde_json::from_slice(&bytes)?;
            if saved!=current_binding{return Err(invalid("backup belongs to another model, BIOS, volume or cache location"))}
            let plan=targets.prepare_restore(source.path(),&backup)?;
            guards.push(source);plan
        },
    };
    let backup_guard=LockedDirectory::open(&backup)?;
    write_new_synced(&backup.join("binding.json"),&serde_json::to_vec_pretty(&current_binding)?)?;
    write_journal(&backup,"prepared",manifest(&plan.desired))?;
    guards.push(backup_guard);
    Ok(NativePlan{plan,backup_id:id,_guards:guards})
}
/// Only the internally discovered fixed directories can create an executable plan.
pub fn prepare_image(image:&Path)->io::Result<NativePlan> { prepare(discovered()?,Action::Image(image)) }
pub fn prepare_factory_reset()->io::Result<NativePlan> { prepare(discovered()?,Action::Factory) }
pub fn prepare_backup_restore(id:&str)->io::Result<NativePlan> {
    if !valid_id(id){return Err(invalid("invalid backup ID"))}
    prepare(discovered()?,Action::Restore(id))
}
impl NativePlan {
    pub fn preview(&self)->PlanPreview { self.plan.preview() }
    pub fn backup_id(&self)->&str { &self.backup_id }
    pub fn execute(self)->io::Result<NativeReport> {
        let backup=self.plan.backup.clone();
        let desired=manifest(&self.plan.desired);
        write_journal(&backup,"applying",manifest(&self.plan.desired))?;
        let transaction=match self.plan.commit(None) {
            Ok(report)=>report,
            Err(e)=>{let _=write_journal(&backup,"not_applied",desired);return Err(e)},
        };
        let state=if transaction.applied {"applied"}
            else if transaction.rollback.len()==4&&transaction.rollback.iter().all(|(_,r)|r.is_ok()) {"rolled_back"}
            else {"recovery_required"};
        let journal_error=write_journal(&backup,state,desired).err().map(|e|e.to_string());
        Ok(NativeReport{backup_id:self.backup_id,transaction,journal_error})
    }
}
/// Read-only inventory; an interrupted `applying` entry stays visible for recovery.
pub fn list_backups()->io::Result<Vec<BackupSummary>> {
    let discovery=discovered()?;
    let root=match LockedDirectory::open(&backup_root(&discovery)?) {Ok(v)=>v,Err(e) if e.kind()==io::ErrorKind::NotFound=>return Ok(vec![]),Err(e)=>return Err(e)};
    let mut results=Vec::new();
    for entry in fs::read_dir(root.path())? {
        let entry=entry?;let id=entry.file_name().to_string_lossy().to_string();
        if !valid_id(&id){continue}
        let state=(||->io::Result<String>{
            let guard=LockedDirectory::open(&entry.path())?;
            let bytes=read(&guard.path().join("transaction.json"))?.ok_or_else(||invalid("missing transaction journal"))?;
            let journal:Journal=serde_json::from_slice(&bytes)?;
            if journal.schema!=1{return Err(invalid("unsupported transaction journal"))}
            Ok(journal.state)
        })().unwrap_or_else(|_|"incomplete".into());
        results.push(BackupSummary{id,state});
    }
    results.sort_by(|a,b|b.id.cmp(&a.id));Ok(results)
}

#[cfg(all(test,windows))]mod tests {
    use super::*;
    fn discovery(root:&Path)->Discovery {
        Discovery{model:"Predator PHN16-71".into(),bios:"V1.18".into(),esp_volume:"test-volume".into(),
            esp_oem:root.join("esp").join("EFI").join("OEM"),cache:root.join("program-data").join("oem").join("AcerAgentService").join("BIOSLogo"),display_pixels:[1920,1080],maximum_image_pixels:[768,432]}
    }
    #[test]fn temporary_native_transaction_and_bound_restore(){
        let root=std::env::temp_dir().join(format!("acer-native-logo-{}",next_id()));
        fs::create_dir(&root).unwrap();let d=discovery(&root);
        assert_eq!(backup_root(&d).unwrap(),root.join("program-data").join("AcerConsole").join("BootLogoBackups"));
        fs::create_dir_all(&d.esp_oem).unwrap();fs::create_dir_all(&d.cache).unwrap();
        fs::write(d.esp_oem.join("AcerLogo.gif"),b"existing override").unwrap();
        let plan=prepare(d.clone(),Action::Factory).unwrap();let saved_id=plan.backup_id().to_string();
        let result=plan.execute().unwrap();assert!(result.transaction.applied);assert!(result.journal_error.is_none());
        assert!(!d.esp_oem.join("AcerLogo.gif").exists());
        let restored=prepare(d.clone(),Action::Restore(&saved_id)).unwrap().execute().unwrap();
        assert!(restored.transaction.applied);assert_eq!(read(&d.esp_oem.join("AcerLogo.gif")).unwrap().unwrap(),b"existing override");
        let mut different=d.clone();different.esp_volume="other-volume".into();
        assert!(prepare(different,Action::Restore(&saved_id)).is_err());
        fs::remove_dir_all(&root).unwrap();
    }
    #[test]fn corrupted_image_and_changed_target_do_not_commit(){
        let root=std::env::temp_dir().join(format!("acer-native-validation-{}",next_id()));
        fs::create_dir(&root).unwrap();let d=discovery(&root);
        let image=root.join("source.gif");fs::write(&image,b"GIF89a\x01\0\x01\0\0\0\0;").unwrap();
        assert!(prepare(d.clone(),Action::Image(&image)).is_err());
        assert_eq!(fs::read_dir(backup_root(&d).unwrap()).unwrap().count(),0);
        let mut encoded=Vec::new();
        {let mut encoder=image::codecs::gif::GifEncoder::new(&mut encoded);
        encoder.encode_frame(image::Frame::new(image::RgbaImage::from_pixel(2,2,image::Rgba([12,34,56,255])))).unwrap();}
        fs::write(&image,&encoded).unwrap();
        let plan=prepare(d.clone(),Action::Image(&image)).unwrap();
        fs::write(&image,b"source changed after review").unwrap();
        assert!(plan.execute().unwrap().transaction.applied);
        assert_eq!(read(&d.esp_oem.join("AcerLogo.gif")).unwrap().unwrap(),encoded);
        assert_eq!(read(&d.cache.join("AcerLogo.gif")).unwrap().unwrap(),encoded);
        let plan=prepare(d.clone(),Action::Factory).unwrap();
        fs::write(d.esp_oem.join("AcerLogo.gif"),b"external edit").unwrap();
        assert!(plan.execute().is_err());
        assert_eq!(read(&d.esp_oem.join("AcerLogo.gif")).unwrap().unwrap(),b"external edit");
        fs::remove_dir_all(root).unwrap();
    }
    #[test]fn journal_failure_before_commit_keeps_targets(){
        let root=std::env::temp_dir().join(format!("acer-native-journal-{}",next_id()));
        fs::create_dir(&root).unwrap();let d=discovery(&root);
        fs::create_dir_all(&d.esp_oem).unwrap();fs::write(d.esp_oem.join("AcerLogo.gif"),b"original").unwrap();
        let plan=prepare(d.clone(),Action::Factory).unwrap();
        fs::write(plan.preview().backup.join("transaction.pending"),b"occupied").unwrap();
        assert!(plan.execute().is_err());
        assert_eq!(read(&d.esp_oem.join("AcerLogo.gif")).unwrap().unwrap(),b"original");
        fs::remove_dir_all(root).unwrap();
    }
}
