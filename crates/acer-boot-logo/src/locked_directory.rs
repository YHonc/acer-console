//! Pins an existing directory and its ancestors against rename and reparse replacement.
use std::{io, path::{Path, PathBuf}};

#[derive(Debug)]
pub(crate) struct LockedDirectory {
    path: PathBuf,
    #[cfg(windows)]
    canonical_guid_path: PathBuf,
    #[cfg(windows)]
    _handles: Vec<std::fs::File>,
}

impl LockedDirectory {
    pub(crate) fn path(&self) -> &Path { &self.path }
    #[cfg(windows)]
    pub(crate) fn canonical_guid_path(&self) -> &Path { &self.canonical_guid_path }

    #[cfg(not(windows))]
    pub(crate) fn open(_: &Path) -> io::Result<Self> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "directory locking requires Windows"))
    }

    #[cfg(windows)]
    pub(crate) fn open(path: &Path) -> io::Result<Self> {
        use std::{fs::OpenOptions, os::windows::{fs::{OpenOptionsExt, MetadataExt}, io::AsRawHandle}, path::{Component, Prefix}};
        let invalid = || io::Error::new(io::ErrorKind::InvalidInput, "expected an absolute normal Windows directory path");
        let mut components = path.components();
        let prefix = match components.next() { Some(Component::Prefix(p)) => p, _ => return Err(invalid()) };
        match prefix.kind() {
            Prefix::Disk(_) | Prefix::VerbatimDisk(_) => {},
            Prefix::Verbatim(name) if name.to_string_lossy().starts_with("Volume{") && name.to_string_lossy().ends_with('}') => {},
            _ => return Err(invalid()),
        }
        if components.next() != Some(Component::RootDir) { return Err(invalid()); }
        let mut current = PathBuf::from(prefix.as_os_str());
        current.push("\\");
        let mut paths = vec![current.clone()];
        for part in components {
            match part { Component::Normal(name) => current.push(name), _ => return Err(invalid()) }
            paths.push(current.clone());
        }
        let mut handles = Vec::new();
        for directory in paths {
            let file = OpenOptions::new().read(true).share_mode(3)
                .custom_flags(0x0020_0000 | 0x0200_0000).open(&directory)?;
            let metadata = file.metadata()?;
            if !metadata.is_dir() || metadata.file_attributes() & 0x400 != 0 {
                return Err(io::Error::new(io::ErrorKind::InvalidInput, "directory chain contains a reparse point or non-directory"));
            }
            handles.push(file);
        }
        // A volume GUID path establishes the identity of the pinned final handle.
        #[link(name = "kernel32")]
        unsafe extern "system" { fn GetFinalPathNameByHandleW(h: *mut std::ffi::c_void, p: *mut u16, n: u32, flags: u32) -> u32; }
        let handle = handles.last().unwrap().as_raw_handle();
        let required = unsafe { GetFinalPathNameByHandleW(handle, std::ptr::null_mut(), 0, 1) };
        if required == 0 { return Err(io::Error::last_os_error()); }
        let mut buffer = vec![0u16; required as usize + 1];
        let length = unsafe { GetFinalPathNameByHandleW(handle, buffer.as_mut_ptr(), buffer.len() as u32, 1) };
        if length == 0 { return Err(io::Error::last_os_error()); }
        if length as usize >= buffer.len() || !String::from_utf16_lossy(&buffer[..length as usize]).starts_with("\\\\?\\Volume{") {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "could not establish volume GUID directory identity"));
        }
        use std::os::windows::ffi::OsStringExt;
        let canonical_guid_path = PathBuf::from(std::ffi::OsString::from_wide(&buffer[..length as usize]));
        if matches!(prefix.kind(), Prefix::Verbatim(_)) {
            let expected = path.as_os_str().to_string_lossy().trim_end_matches('\\').to_lowercase();
            let actual = canonical_guid_path.as_os_str().to_string_lossy().trim_end_matches('\\').to_lowercase();
            if expected != actual { return Err(io::Error::new(io::ErrorKind::InvalidData, "volume GUID directory identity differs from requested path")); }
        }
        Ok(Self { path: path.to_path_buf(), canonical_guid_path, _handles: handles })
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    #[test]
    fn locked_directory_rejects_parent_junction() {
        let root = std::env::temp_dir().join(format!("acer-junction-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let target = root.join("target");
        let link = root.join("link");
        std::fs::create_dir_all(target.join("child")).unwrap();
        let status = std::process::Command::new("cmd.exe").args(["/d", "/c", "mklink", "/J"]).arg(&link).arg(&target).status().unwrap();
        assert!(status.success(), "temporary junction creation failed");
        let result = LockedDirectory::open(&link.join("child"));
        // Remove only the junction entry before cleaning up the ordinary tree.
        std::fs::remove_dir(&link).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        assert!(result.is_err());
    }
    #[test]
    fn locked_directory_blocks_rename_until_drop() {
        let root = std::env::temp_dir().join(format!("acer-directory-lock-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let child = root.join("a").join("b");
        std::fs::create_dir_all(&child).unwrap();
        let guard = LockedDirectory::open(&child).unwrap();
        assert_eq!(guard.path(), child);
        let renamed = root.with_extension("renamed");
        assert!(std::fs::rename(&root, &renamed).is_err());
        drop(guard);
        std::fs::rename(&root, &renamed).unwrap();
        std::fs::remove_dir_all(renamed).unwrap();
    }
}
