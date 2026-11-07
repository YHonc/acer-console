//! Pin a local directory chain while native recovery records are in use.
use std::{fs,io,path::{Path,PathBuf}};
pub struct DirectoryGuard {path:PathBuf,_handles:Vec<fs::File>}
impl DirectoryGuard{
    pub fn path(&self)->&Path{&self.path}
    #[cfg(windows)]
    pub fn open(path:&Path)->io::Result<Self>{
        use std::{os::windows::fs::{OpenOptionsExt,MetadataExt},path::{Component,Prefix}};
        let invalid=||io::Error::new(io::ErrorKind::InvalidInput,"Expected a local absolute directory without reparse points");
        let mut parts=path.components();let prefix=match parts.next(){Some(Component::Prefix(p))if matches!(p.kind(),Prefix::Disk(_)|Prefix::VerbatimDisk(_))=>p,_=>return Err(invalid())};
        if parts.next()!=Some(Component::RootDir){return Err(invalid())}
        let mut current=PathBuf::from(prefix.as_os_str());current.push("\\");let mut paths=vec![current.clone()];
        for part in parts{if let Component::Normal(name)=part{current.push(name);paths.push(current.clone())}else{return Err(invalid())}}
        let mut handles=vec![];
        for directory in paths{
            let file=fs::OpenOptions::new().read(true).share_mode(3).custom_flags(0x0020_0000|0x0200_0000).open(directory)?;
            let meta=file.metadata()?;if !meta.is_dir()||meta.file_attributes()&0x400!=0{return Err(invalid())}handles.push(file);
        }
        Ok(Self{path:path.to_path_buf(),_handles:handles})
    }
    #[cfg(not(windows))]
    pub fn open(_: &Path)->io::Result<Self>{Err(io::Error::new(io::ErrorKind::Unsupported,"Directory guards require Windows"))}
    pub fn create(path:&Path)->io::Result<Self>{
        match Self::open(path){Ok(guard)=>Ok(guard),Err(e)if e.kind()==io::ErrorKind::NotFound=>{
            let parent=path.parent().ok_or_else(||io::Error::new(io::ErrorKind::InvalidInput,"Missing directory parent"))?;
            let _parent=Self::create(parent)?;
            match fs::create_dir(path){Ok(())=>(),Err(e)if e.kind()==io::ErrorKind::AlreadyExists=>(),Err(e)=>return Err(e)}
            Self::open(path)
        },Err(e)=>Err(e)}
    }
}
/// Bounded callers still validate size and JSON. Reject reparse files and hold
/// off replacement/writes while reading a record.
pub fn open_record(path:&Path)->io::Result<fs::File>{
    let mut options=fs::OpenOptions::new();options.read(true);
    #[cfg(windows)]{use std::os::windows::fs::OpenOptionsExt;options.share_mode(1).custom_flags(0x0020_0000);}
    let file=options.open(path)?;let meta=file.metadata()?;
    if !meta.is_file(){return Err(io::Error::new(io::ErrorKind::InvalidInput,"Recovery record is not a file"))}
    #[cfg(windows)]{use std::os::windows::fs::MetadataExt;if meta.file_attributes()&0x400!=0{return Err(io::Error::new(io::ErrorKind::InvalidInput,"Recovery record is a reparse point"))}}
    Ok(file)
}
#[cfg(all(test,windows))]mod tests{
    use super::*;
    fn root()->PathBuf{std::env::temp_dir().join(format!("acer-record-guard-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()))}
    #[test]fn created_chain_cannot_be_renamed_until_released(){
        let root=root();let guard=DirectoryGuard::create(&root.join("a").join("b")).unwrap();assert!(fs::rename(&root,root.with_extension("moved")).is_err());
        drop(guard);fs::rename(&root,root.with_extension("moved")).unwrap();fs::remove_dir_all(root.with_extension("moved")).unwrap();
    }
    #[test]fn junction_parent_is_rejected_without_writing_through_it(){
        let root=root();fs::create_dir_all(root.join("real")).unwrap();
        let status=std::process::Command::new("cmd.exe").args(["/d","/c","mklink","/J"]).arg(root.join("link")).arg(root.join("real")).status().unwrap();assert!(status.success());
        assert!(DirectoryGuard::create(&root.join("link").join("new")).is_err());assert!(!root.join("real").join("new").exists());
        fs::remove_dir(root.join("link")).unwrap();fs::remove_dir_all(root).unwrap();
    }
    #[test]fn reading_record_blocks_concurrent_write_and_replacement(){
        let root=root();fs::create_dir(&root).unwrap();let path=root.join("record.json");fs::write(&path,b"{}").unwrap();let file=open_record(&path).unwrap();
        assert!(fs::write(&path,b"changed").is_err());assert!(fs::rename(&path,root.join("moved.json")).is_err());drop(file);fs::remove_dir_all(root).unwrap();
    }
}
