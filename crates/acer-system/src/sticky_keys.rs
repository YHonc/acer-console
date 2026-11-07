//! Interactive-user StickyKeys, using the same 0x5 flag mask as the local vendor.
//! No service impersonation, registry write, persistence, or automatic elevation.
use crate::{error, SystemError};
use serde::Serialize;
const CONTROL_MASK: u32 = 0x5;
const CONFIG_MASK: u32 = 0xffff;
static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[derive(Debug, Clone, Copy, Serialize)]
pub struct State {
    pub enabled: bool,
    pub hotkey_active: bool,
    pub available: bool,
    pub flags: u32,
}
impl State {
    fn from_flags(flags: u32) -> Self {
        Self { enabled: flags & 1 != 0, hotkey_active: flags & 4 != 0, available: flags & 2 != 0, flags }
    }
}
fn target_flags(flags: u32, enabled: bool) -> u32 {
    if enabled { flags | CONTROL_MASK } else { flags & !CONTROL_MASK }
}

#[cfg(windows)]
fn call(action: u32, flags: u32) -> Result<State, SystemError> {
    #[repr(C)] struct StickyKeys { size: u32, flags: u32 }
    #[link(name = "user32")]
    extern "system" { fn SystemParametersInfoW(action: u32, param: u32, data: *mut std::ffi::c_void, flags: u32) -> i32; }
    let mut data = StickyKeys { size: std::mem::size_of::<StickyKeys>() as u32, flags };
    // The pointer is aligned and valid for the synchronous call; fWinIni=0 matches
    // the vendor operation and deliberately does not persist a test to the profile.
    if unsafe { SystemParametersInfoW(action, data.size, (&mut data as *mut StickyKeys).cast(), 0) } == 0 {
        return Err(error(std::io::Error::last_os_error()));
    }
    Ok(State::from_flags(data.flags))
}
#[cfg(not(windows))]
fn call(_: u32, _: u32) -> Result<State, SystemError> { Err(error("StickyKeys requires Windows")) }

pub fn get() -> Result<State, SystemError> { call(0x3a, 0) }
fn write_flags(flags: u32) -> Result<State, SystemError> {
    call(0x3b, flags)?;
    let after = get()?;
    if after.flags & CONFIG_MASK != flags & CONFIG_MASK {
        return Err(error("StickyKeys configuration readback differs from requested flags"));
    }
    Ok(after)
}
pub fn set(enabled: bool) -> Result<State, SystemError> {
    let _guard = LOCK.lock().map_err(error)?;
    let before = get()?;
    if !before.available { return Err(error("StickyKeys is unavailable in this user session")); }
    write_flags(target_flags(before.flags, enabled))
}

#[derive(Debug, Serialize)]
pub struct Validation {
    pub before: State,
    pub trial: Result<State, SystemError>,
    pub restore: Result<State, SystemError>,
    pub restored: bool,
}
/// Native acceptance probe only. Always restores the captured configuration after
/// attempting the trial, including when the setter or readback reports failure.
pub fn validate_restore() -> Result<Validation, SystemError> {
    let _guard = LOCK.lock().map_err(error)?;
    let before = get()?;
    if !before.available { return Err(error("StickyKeys is unavailable in this user session")); }
    let trial = write_flags(target_flags(before.flags, !before.enabled));
    let restore = write_flags(before.flags);
    let restored = restore.as_ref().is_ok_and(|s| s.flags & CONFIG_MASK == before.flags & CONFIG_MASK);
    Ok(Validation { before, trial, restore, restored })
}

#[cfg(test)] mod tests {
    use super::*;
    #[test] fn changes_only_vendor_control_bits_and_preserves_options() {
        for flags in [0, 2, 0x1fa, 0x1fe, 0x10001ff, u32::MAX] {
            for enabled in [false, true] {
                let next = target_flags(flags, enabled);
                assert_eq!(next & !CONTROL_MASK, flags & !CONTROL_MASK);
                assert_eq!(next & CONTROL_MASK, if enabled { CONTROL_MASK } else { 0 });
            }
        }
    }
    #[test] fn hotkey_only_does_not_claim_feature_enabled() {
        let state = State::from_flags(0x1fe);
        assert!(!state.enabled); assert!(state.hotkey_active); assert!(state.available);
    }
}
