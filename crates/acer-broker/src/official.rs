//! Fixed system integration actions, invoked only after pipe peer authentication.
use super::*;
use std::path::{Path,PathBuf};

fn config_under(roaming:&Path)->Result<PathBuf,DeviceError> {
    if !roaming.is_absolute() {return Err(error("RoamingAppData path is not absolute"));}
    Ok(roaming.join("local.phn16.acerconsole"))
}

#[cfg(windows)]
pub(crate) fn config_directory()->Result<PathBuf,DeviceError> {
    use std::{ffi::{c_void,OsString},os::windows::ffi::OsStringExt,ptr::null_mut};
    #[repr(C)] struct Guid {a:u32,b:u16,c:u16,d:[u8;8]}
    // Windows SDK KnownFolders.h: FOLDERID_RoamingAppData.
    const ROAMING:Guid=Guid{a:0x3eb685db,b:0x65f9,c:0x4cf6,d:[0xa0,0x3a,0xe3,0xef,0x65,0x72,0x9f,0x3d]};
    #[link(name="shell32")] unsafe extern "system" {
        fn SHGetKnownFolderPath(id:*const Guid,flags:u32,token:*mut c_void,path:*mut *mut u16)->i32;
    }
    #[link(name="ole32")] unsafe extern "system" {fn CoTaskMemFree(memory:*mut c_void);}
    struct Allocated(*mut u16);
    impl Drop for Allocated {fn drop(&mut self){unsafe{CoTaskMemFree(self.0.cast());}}}
    let mut raw=null_mut();
    let result=unsafe{SHGetKnownFolderPath(&ROAMING,0,null_mut(),&mut raw)};
    let allocated=Allocated(raw);
    if result<0 || raw.is_null(){return Err(error(format!("SHGetKnownFolderPath failed: {result:#x}")));}
    let mut length=0;
    unsafe{while *allocated.0.add(length)!=0 {length+=1;}}
    let path=PathBuf::from(OsString::from_wide(unsafe{std::slice::from_raw_parts(allocated.0,length)}));
    config_under(&path)
}

#[cfg(windows)]
pub(crate) fn execute(action:OfficialAction)->Result<serde_json::Value,DeviceError> {
    // Elevation uses the SAME user SID/session checked by platform::serve.
    // Resolve this account through the shell API, never inherited APPDATA or RPC JSON.
    let directory=config_directory()?;
    let result=match action {
        OfficialAction::Query=>Ok(()),
        OfficialAction::Backup=>acer_system::backup_official_task(&directory).map(|_|()),
        OfficialAction::Disable=>acer_system::disable_official_task(&directory).map(|_|()),
        OfficialAction::Restore=>acer_system::restore_official_task(&directory).map(|_|()),
    };
    let mut failure=result.err().map(|e|e.to_string());
    let state=match acer_system::get_official_task() {
        Ok(state)=>Some(state),
        Err(err)=>{let message=err.to_string();failure=Some(match failure {Some(old)=>format!("{old}; readback: {message}"),None=>message});None},
    };
    let backup=acer_system::get_official_backup(&directory);
    // Keep top-level task fields compatible with the existing desktop response.
    let state_value=serde_json::to_value(&state).map_err(error)?;
    let mut response=state_value.as_object().cloned().unwrap_or_default();
    response.insert("state".into(),state_value);
    response.insert("backup_valid".into(),backup.backup_valid.into());
    response.insert("original_enabled".into(),serde_json::json!(backup.backup_original_enabled));
    response.insert("backup_original_enabled".into(),serde_json::json!(backup.backup_original_enabled));
    response.insert("backup_error".into(),serde_json::json!(backup.backup_error));
    response.insert("error".into(),serde_json::json!(failure));
    Ok(serde_json::Value::Object(response))
}

#[cfg(test)] mod tests {
    use super::*;
    #[test] fn backup_directory_has_only_fixed_application_suffix() {
        #[cfg(windows)] let base=Path::new(r"C:\Users\example\AppData\Roaming");
        #[cfg(not(windows))] let base=Path::new("/home/example/roaming");
        assert_eq!(config_under(base).unwrap(),base.join("local.phn16.acerconsole"));
        assert!(config_under(Path::new("relative")).is_err());
    }
    #[cfg(windows)]
    #[test] fn known_folder_query_is_read_only_and_absolute() {
        let path=config_directory().unwrap();
        assert!(path.is_absolute());
        assert!(path.ends_with("local.phn16.acerconsole"));
    }
}
