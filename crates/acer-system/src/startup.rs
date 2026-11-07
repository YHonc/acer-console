use super::*;
use std::{ffi::c_void, ptr::{null, null_mut}};
type Key = *mut c_void;
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const NAME: &str = "AcerConsole";
fn hkcu() -> Key { 0x80000001u32 as i32 as isize as Key }
#[link(name="advapi32")] unsafe extern "system" {
    fn RegGetValueW(key: Key, subkey: *const u16, value: *const u16, flags: u32, kind: *mut u32, data: *mut c_void, size: *mut u32) -> i32;
    fn RegCreateKeyExW(key: Key, subkey: *const u16, reserved: u32, class: *mut u16, options: u32, access: u32, security: *const c_void, result: *mut Key, disposition: *mut u32) -> i32;
    fn RegOpenKeyExW(key: Key, subkey: *const u16, options: u32, access: u32, result: *mut Key) -> i32;
    fn RegSetValueExW(key: Key, name: *const u16, reserved: u32, kind: u32, data: *const u8, size: u32) -> i32;
    fn RegDeleteValueW(key: Key, name: *const u16) -> i32;
    fn RegCloseKey(key: Key) -> i32;
}
struct OwnedKey(Key);
impl Drop for OwnedKey { fn drop(&mut self) { unsafe { RegCloseKey(self.0); } } }
fn wide(s: &str) -> Vec<u16> { s.encode_utf16().chain(Some(0)).collect() }
fn status(code: i32) -> Result<(), SystemError> { if code == 0 { Ok(()) } else { Err(error(std::io::Error::from_raw_os_error(code))) } }
fn command_for(exe: &Path) -> Result<String, SystemError> {
    let text = exe.to_str().ok_or_else(||error("Executable path is not Unicode"))?;
    if !exe.is_absolute() || !exe.file_name().and_then(|v|v.to_str()).is_some_and(|v|v.eq_ignore_ascii_case("acer-console.exe")) || text.contains(['"','\0']) { return Err(error("Startup can only register the running acer-console.exe")); }
    let command = format!("\"{text}\" --tray");
    if command.encode_utf16().count() > 260 { return Err(error("HKCU Run command exceeds the Windows 260-character limit")); }
    Ok(command)
}
fn expected() -> Result<String, SystemError> { command_for(&std::env::current_exe().map_err(error)?) }
pub(super) fn get() -> Result<StartupState, SystemError> {
    let expected = expected()?;
    let path = wide(RUN_KEY); let name = wide(NAME); let mut size = 0; let mut kind = 0;
    // No type filter: an unexpected existing type is reported as mismatch, not absence.
    let code = unsafe { RegGetValueW(hkcu(),path.as_ptr(),name.as_ptr(),0x1000ffff,&mut kind,null_mut(),&mut size) };
    if code == 2 { return Ok(StartupState { enabled:false, entry_present:false, mismatch:false }); }
    status(code)?;
    if kind != 1 || size == 0 || size > 65536 || size % 2 != 0 { return Ok(StartupState { enabled:false, entry_present:true, mismatch:true }); }
    let mut data = vec![0u16; size as usize / 2];
    status(unsafe { RegGetValueW(hkcu(),path.as_ptr(),name.as_ptr(),0x10000002,&mut kind,data.as_mut_ptr().cast(),&mut size) })?;
    let content = &data[..size as usize / 2];
    let command = String::from_utf16(content.strip_suffix(&[0]).unwrap_or(content)).map_err(error)?;
    let matches = command.eq_ignore_ascii_case(&expected);
    Ok(StartupState { enabled:matches, entry_present:true, mismatch:!matches })
}
pub(super) fn set(enabled: bool) -> Result<StartupState, SystemError> {
    let command = expected()?; // Validate actual host even when disabling.
    let path = wide(RUN_KEY); let name = wide(NAME); let mut key = null_mut();
    if enabled {
        status(unsafe { RegCreateKeyExW(hkcu(),path.as_ptr(),0,null_mut(),0,2,null(),&mut key,null_mut()) })?;
        let key = OwnedKey(key); let value = wide(&command);
        status(unsafe { RegSetValueExW(key.0,name.as_ptr(),0,1,value.as_ptr().cast(),(value.len()*2) as u32) })?;
    } else {
        let code = unsafe { RegOpenKeyExW(hkcu(),path.as_ptr(),0,2,&mut key) };
        if code != 2 {
            status(code)?; let key=OwnedKey(key);
            let code=unsafe { RegDeleteValueW(key.0,name.as_ptr()) };
            if code != 2 { status(code)?; }
        }
    }
    let state=get()?;
    if state.enabled != enabled || (!enabled && state.entry_present) { return Err(error("Startup registration readback did not match requested state")); }
    Ok(state)
}
#[cfg(test)] mod tests {
    use super::*;
    #[test] fn command_is_fixed_and_quoted() {
        assert_eq!(command_for(Path::new(r"C:\Program Files\Acer Console\acer-console.exe")).unwrap(),r#""C:\Program Files\Acer Console\acer-console.exe" --tray"#);
        assert!(command_for(Path::new(r"C:\Windows\cmd.exe")).is_err());
        assert!(command_for(Path::new("acer-console.exe")).is_err());
    }
}
