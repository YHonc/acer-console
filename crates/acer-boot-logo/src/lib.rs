//! Fixed-slot boot logo backup and transaction core.
//! Arbitrary target plans remain read/prepare-only; native transactions require discovery.
use serde::{Deserialize, Serialize};
pub mod native_discovery;
pub mod image_validation;
mod locked_directory;
pub mod native_transaction;
use sha2::{Digest, Sha256};
use std::{fs, io, path::{Path, PathBuf}};

const LIMIT: u64 = 35_000_000;
const NAMES: [&str; 2] = ["AcerLogo.gif", "AcerLogo.jpg"];
const SLOTS: [&str; 4] = ["esp-gif", "esp-jpg", "cache-gif", "cache-jpg"];
fn invalid(message: &str) -> io::Error { io::Error::new(io::ErrorKind::InvalidData, message) }
fn hash(bytes: &[u8]) -> String { format!("{:x}", Sha256::digest(bytes)) }
fn directory(path: &Path) -> io::Result<PathBuf> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() || is_link(&metadata) { return Err(invalid("unsafe directory")); }
    canonical_directory(path)
}
#[cfg(not(windows))]
fn canonical_directory(path:&Path)->io::Result<PathBuf>{fs::canonicalize(path)}
#[cfg(windows)]
fn canonical_directory(path:&Path)->io::Result<PathBuf>{
    use std::{ffi::{c_void,OsString},os::windows::{ffi::OsStringExt,fs::OpenOptionsExt,io::AsRawHandle}};
    #[link(name="kernel32")]extern "system"{fn GetFinalPathNameByHandleW(handle:*mut c_void,path:*mut u16,size:u32,flags:u32)->u32;}
    let file=fs::OpenOptions::new().read(true).custom_flags(0x02000000|0x00200000).open(path)?;
    // A hidden ESP has no DOS drive letter. Ask Windows for its canonical volume-GUID path.
    let required=unsafe{GetFinalPathNameByHandleW(file.as_raw_handle(),std::ptr::null_mut(),0,1)};
    if required==0{return Err(io::Error::last_os_error())}
    let mut buffer=vec![0u16;required as usize+1];
    let length=unsafe{GetFinalPathNameByHandleW(file.as_raw_handle(),buffer.as_mut_ptr(),buffer.len()as u32,1)};
    if length==0{return Err(io::Error::last_os_error())}
    if length as usize>=buffer.len(){return Err(invalid("directory path changed during discovery"))}
    Ok(PathBuf::from(OsString::from_wide(&buffer[..length as usize])))
}
fn directory_or_missing(path: &Path) -> io::Result<PathBuf> {
    if !path.is_absolute() || path.components().any(|part|matches!(part,std::path::Component::ParentDir|std::path::Component::CurDir)) {return Err(invalid("target directory must be an absolute normal path"));}
    match fs::symlink_metadata(path) {
        Ok(_) => directory(path),
        Err(error) if error.kind()==io::ErrorKind::NotFound => {
            let parent=path.parent().ok_or_else(||invalid("target volume does not exist"))?;
            let name=path.file_name().ok_or_else(||invalid("target directory has no final component"))?;
            Ok(directory_or_missing(parent)?.join(name))
        },
        Err(error)=>Err(error),
    }
}
fn is_link(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)] { use std::os::windows::fs::MetadataExt; metadata.file_attributes() & 0x400 != 0 }
    #[cfg(not(windows))] { metadata.file_type().is_symlink() }
}
fn read(path: &Path) -> io::Result<Option<Vec<u8>>> {
    use std::io::Read;
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)] {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(1).custom_flags(0x00200000);
    }
    #[cfg(not(windows))]
    if fs::symlink_metadata(path).is_ok_and(|m| is_link(&m)) { return Err(invalid("unsafe file link")); }
    let mut file = match options.open(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        result => result?,
    };
    let metadata = file.metadata()?;
    if !metadata.is_file() || is_link(&metadata) || metadata.len() > LIMIT { return Err(invalid("unsafe or oversized file")); }
    let mut bytes = Vec::new();
    (&mut file).take(LIMIT + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > LIMIT { return Err(invalid("file grew beyond limit")); }
    Ok(Some(bytes))
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry { pub slot: String, pub present: bool, pub sha256: Option<String> }
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest { pub entries: Vec<Entry> }
pub struct Targets { dirs: [PathBuf; 2] }
pub struct Plan {
    targets: Targets,
    original: [Option<Vec<u8>>; 4],
    desired: [Option<Vec<u8>>; 4],
    backup: PathBuf,
}
#[derive(Debug,Serialize)]
pub struct Report {
    pub applied: bool,
    pub failures: Vec<String>,
    pub rollback: Vec<(usize, Result<(), String>)>,
}
#[derive(Debug,Serialize)]
pub struct PlanPreview {
    pub targets:Vec<PathBuf>,
    pub before:Manifest,
    pub after:Manifest,
    pub backup:PathBuf,
}
impl Targets {
    pub fn inspect(esp_oem: &Path, cache: &Path) -> io::Result<Self> {
        Ok(Self { dirs: [directory_or_missing(esp_oem)?, directory_or_missing(cache)?] })
    }
    fn path(&self, index: usize) -> PathBuf { self.dirs[index / 2].join(NAMES[index % 2]) }
    fn snapshot(&self) -> io::Result<[Option<Vec<u8>>; 4]> {
        let mut bytes: [Option<Vec<u8>>; 4] = Default::default();
        for index in 0..4 { bytes[index] = read(&self.path(index))?; }
        Ok(bytes)
    }
    pub fn states(&self) -> io::Result<Manifest> { Ok(manifest(&self.snapshot()?)) }
    pub fn prepare(self, image: Option<&Path>, resolution: (u32, u32), backup: &Path) -> io::Result<Plan> {
        let mut desired: [Option<Vec<u8>>; 4] = Default::default();
        if let Some(path) = image {
            let bytes = read(path)?.ok_or_else(|| invalid("missing image"))?;
            let info = image_validation::validate(&bytes, &decode_limits(
                (u64::from(resolution.0)*2/5) as u32, (u64::from(resolution.1)*2/5) as u32))?;
            let kind=match info.kind {image_validation::ImageKind::Gif=>0,image_validation::ImageKind::Jpeg=>1};
            desired[kind] = Some(bytes.clone());
            desired[kind + 2] = Some(bytes);
        }
        self.prepare_bytes(desired, backup)
    }
    /// Validate persistent backup and prepare restoration to fixed target slots.
    pub fn prepare_restore(self, source: &Path, new_backup: &Path) -> io::Result<Plan> {
        let desired = load_backup(source)?;
        self.prepare_bytes(desired, new_backup)
    }
    fn prepare_bytes(self, desired: [Option<Vec<u8>>; 4], backup: &Path) -> io::Result<Plan> {
        let original = self.snapshot()?;
        fs::create_dir(backup)?;
        let backup = directory(backup)?;
        for index in 0..4 {
            if let Some(bytes) = &original[index] { write_new_synced(&backup.join(SLOTS[index]), bytes)?; }
        }
        // Publish the manifest last: a directory without it is an incomplete backup.
        let temporary=backup.join("manifest.pending");
        write_new_synced(&temporary, &serde_json::to_vec_pretty(&manifest(&original))?)?;
        atomic_replace(&temporary, &backup.join("manifest.json"))?;
        if load_backup(&backup)? != original { return Err(invalid("backup readback mismatch")); }
        Ok(Plan { targets: self, original, desired, backup })
    }
}
fn manifest(bytes: &[Option<Vec<u8>>; 4]) -> Manifest {
    Manifest { entries: (0..4).map(|index| Entry {
        slot: SLOTS[index].into(), present: bytes[index].is_some(),
        sha256: bytes[index].as_ref().map(|bytes| hash(bytes)),
    }).collect() }
}
fn load_backup(source: &Path) -> io::Result<[Option<Vec<u8>>; 4]> {
    let root = directory(source)?;
    let manifest_bytes = read(&root.join("manifest.json"))?.ok_or_else(|| invalid("missing manifest"))?;
    let manifest: Manifest = serde_json::from_slice(&manifest_bytes)?;
    if manifest.entries.len() != 4 { return Err(invalid("manifest requires exactly four slots")); }
    let mut seen = [false; 4];
    let mut result: [Option<Vec<u8>>; 4] = Default::default();
    for entry in manifest.entries {
        let index = SLOTS.iter().position(|slot| *slot == entry.slot).ok_or_else(|| invalid("unknown slot"))?;
        if seen[index] { return Err(invalid("duplicate slot")); }
        seen[index] = true;
        let bytes = read(&root.join(SLOTS[index]))?;
        if entry.present != bytes.is_some() || entry.sha256 != bytes.as_ref().map(|bytes| hash(bytes)) {
            return Err(invalid("backup presence or SHA256 mismatch"));
        }
        result[index] = bytes;
    }
    Ok(result)
}
fn replace(path: &Path, bytes: Option<&[u8]>) -> io::Result<()> {
    use std::io::Write;
    if let Some(bytes) = bytes {
        let temporary = path.with_extension("boot-logo-tmp");
        let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&temporary)?;
        let operation = (|| {
            file.write_all(bytes)?;
            file.sync_all()?;
            drop(file);
            atomic_replace(&temporary, path)
        })();
        let cleanup = fs::remove_file(&temporary);
        if let Err(error) = operation { return Err(error); }
        if let Err(error) = cleanup { if error.kind() != io::ErrorKind::NotFound { return Err(error); } }
    } else {
        match fs::remove_file(path) { Err(error) if error.kind() == io::ErrorKind::NotFound => (), result => result? }
    }
    if read(path)?.as_deref() != bytes { return Err(invalid("write readback mismatch")); }
    Ok(())
}
#[cfg(windows)]
fn atomic_replace(source: &Path, target: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    extern "system" { fn MoveFileExW(source: *const u16, target: *const u16, flags: u32) -> i32; }
    let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let target: Vec<u16> = target.as_os_str().encode_wide().chain(Some(0)).collect();
    if unsafe { MoveFileExW(source.as_ptr(), target.as_ptr(), 0x1 | 0x8) } == 0 { return Err(io::Error::last_os_error()); }
    Ok(())
}
#[cfg(not(windows))]
fn atomic_replace(source: &Path, target: &Path) -> io::Result<()> { fs::rename(source, target) }
impl Plan {
    pub fn preview(&self)->PlanPreview {PlanPreview{targets:(0..4).map(|i|self.targets.path(i)).collect(),before:manifest(&self.original),after:manifest(&self.desired),backup:self.backup.clone()}}
    pub fn backup_directory(&self) -> &Path { &self.backup }
    /// Caller-supplied target paths never grant production write authority.
    pub fn execute(self) -> io::Result<Report> { Err(io::Error::new(io::ErrorKind::PermissionDenied, "execution requires a discovered native plan")) }
    #[cfg(test)]
    fn execute_simulated(self, fail_at: Option<usize>) -> io::Result<Report> { self.commit(fail_at) }
    fn commit(self, fail_at: Option<usize>) -> io::Result<Report> {
        if self.targets.snapshot()? != self.original { return Err(invalid("target changed since preparation")); }
        let mut report = Report { applied: false, failures: vec![], rollback: vec![] };
        for index in 0..4 {
            let operation = replace(&self.targets.path(index), self.desired[index].as_deref()).and_then(|()| {
                if fail_at == Some(index) { Err(invalid("injected readback failure")) } else { Ok(()) }
            });
            if let Err(error) = operation {
                report.failures.push(format!("target {index}: {error}"));
                for slot in 0..4 {
                    report.rollback.push((slot, replace(&self.targets.path(slot), self.original[slot].as_deref()).map_err(|error| error.to_string())));
                }
                return Ok(report);
            }
        }
        report.applied = true;
        Ok(report)
    }
}
fn decode_limits(max_width:u32,max_height:u32)->image_validation::ImageLimits {
    image_validation::ImageLimits { max_width,max_height,max_input_bytes:LIMIT,
        max_frames:4096,max_total_pixels:256_000_000,max_alloc_bytes:64*1024*1024 }
}
fn write_new_synced(path:&Path,bytes:&[u8])->io::Result<()> {
    use std::io::Write;
    let mut file=fs::OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(bytes)?; file.sync_all()
}
/// Fully decode a bounded GIF/JPEG. Preparation additionally uses native display dimensions.
pub fn image_info(bytes: &[u8]) -> io::Result<(usize, u32, u32)> {
    let info=image_validation::validate(bytes,&decode_limits(8192,8192))?;
    Ok((match info.kind {image_validation::ImageKind::Gif=>0,image_validation::ImageKind::Jpeg=>1},info.width,info.height))
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (PathBuf, Targets) {
        let root = std::env::temp_dir().join(format!("acer-logo-test-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        fs::create_dir_all(root.join("esp")).unwrap(); fs::create_dir(root.join("cache")).unwrap();
        fs::write(root.join("esp/AcerLogo.gif"), b"original").unwrap();
        let targets = Targets::inspect(&root.join("esp"), &root.join("cache")).unwrap();
        (root, targets)
    }
    #[test]
    fn persistent_restore_and_rollback() {
        let (root, targets) = fixture();
        let plan = targets.prepare(None, (1920,1080), &root.join("backup")).unwrap();
        let report = plan.execute_simulated(Some(2)).unwrap();
        assert!(!report.applied); assert!(report.rollback.iter().all(|(_, result)| result.is_ok()));
        assert_eq!(fs::read(root.join("esp/AcerLogo.gif")).unwrap(), b"original");
        let targets = Targets::inspect(&root.join("esp"), &root.join("cache")).unwrap();
        assert!(targets.prepare(None, (1,1), &root.join("backup2")).unwrap().execute_simulated(None).unwrap().applied);
        let targets = Targets::inspect(&root.join("esp"), &root.join("cache")).unwrap();
        assert!(targets.prepare_restore(&root.join("backup"), &root.join("backup3")).unwrap().execute_simulated(None).unwrap().applied);
        assert_eq!(fs::read(root.join("esp/AcerLogo.gif")).unwrap(), b"original");
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn rejects_manifest_tampering_and_extra_slots() {
        let (root, targets) = fixture();
        targets.prepare(None, (1,1), &root.join("backup")).unwrap();
        fs::write(root.join("backup/esp-gif"), b"tampered").unwrap();
        assert!(load_backup(&root.join("backup")).is_err());
        fs::write(root.join("backup/manifest.json"), br#"{"entries":[{"slot":"../../evil","present":false,"sha256":null}]}"#).unwrap();
        assert!(load_backup(&root.join("backup")).is_err());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn rejects_truncated_images() {
        assert!(image_info(b"GIF89a\x01\x00\x01\x00").is_err());
        assert!(image_info(&[255,216,255,192,0,20,8,0,1,0,1]).is_err());
    }
}

#[cfg(test)]
mod failure_tests {
    use super::*;
    #[test]
    fn duplicate_missing_unknown_slots_and_false_presence_are_rejected() {
        let root = std::env::temp_dir().join(format!("acer-manifest-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        for slots in [vec!["esp-gif", "esp-gif", "cache-gif", "cache-jpg"], vec!["esp-gif"], vec!["esp-gif", "esp-jpg", "cache-gif", "evil"]] {
            let entries = slots.into_iter().map(|slot| Entry { slot: slot.into(), present: false, sha256: None }).collect();
            fs::write(root.join("manifest.json"), serde_json::to_vec(&Manifest { entries }).unwrap()).unwrap();
            assert!(load_backup(&root).is_err());
        }
        let mut entries = manifest(&Default::default()).entries;
        entries[0].present = true;
        fs::write(root.join("manifest.json"), serde_json::to_vec(&Manifest { entries }).unwrap()).unwrap();
        assert!(load_backup(&root).is_err());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn occupied_temp_keeps_original_and_reports_failed_rollback() {
        let root = std::env::temp_dir().join(format!("acer-temp-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        fs::create_dir(root.join("esp")).unwrap();
        fs::create_dir(root.join("cache")).unwrap();
        fs::write(root.join("esp/AcerLogo.gif"), b"old").unwrap();
        let targets = Targets::inspect(&root.join("esp"), &root.join("cache")).unwrap();
        let mut plan = targets.prepare(None, (1,1), &root.join("backup")).unwrap();
        plan.desired[0] = Some(b"new".to_vec());
        fs::write(root.join("esp/AcerLogo.boot-logo-tmp"), b"occupied").unwrap();
        let report = plan.execute_simulated(None).unwrap();
        assert!(!report.applied);
        assert!(report.rollback[0].1.is_err());
        assert_eq!(fs::read(root.join("esp/AcerLogo.gif")).unwrap(), b"old");
        assert_eq!(fs::read(root.join("esp/AcerLogo.boot-logo-tmp")).unwrap(), b"occupied");
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn arbitrary_target_plan_cannot_execute() {
        let root = std::env::temp_dir().join(format!("acer-disabled-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        fs::create_dir(root.join("esp")).unwrap(); fs::create_dir(root.join("cache")).unwrap();
        let targets = Targets::inspect(&root.join("esp"), &root.join("cache")).unwrap();
        assert_eq!(targets.prepare(None, (1,1), &root.join("backup")).unwrap().execute().unwrap_err().kind(), io::ErrorKind::PermissionDenied);
        fs::remove_dir_all(root).unwrap();
    }
}
