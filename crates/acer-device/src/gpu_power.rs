//! NVML configured power limits, in integer milliwatts. Not Dynamic Boost/TGP telemetry.
//! No entry point is connected to the normal device RPC or desktop UI.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Error { pub operation: String, pub code: Option<u32>, pub message: String }
impl Error {
    pub fn local(operation: &str, message: impl Into<String>) -> Self {
        Self { operation: operation.into(), code: None, message: message.into() }
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {} ({:?})", self.operation, self.message, self.code)
    }
}
impl std::error::Error for Error {}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct Limits { pub min_mw: u32, pub max_mw: u32 }
impl Limits {
    fn contains(self, value: u32) -> bool {
        self.min_mw > 0 && self.min_mw <= self.max_mw && value >= self.min_mw && value <= self.max_mw
    }
}
pub trait PowerIo {
    fn configured_limit(&mut self) -> Result<u32, Error>;
    fn limits(&mut self) -> Result<Limits, Error>;
    fn set_limit(&mut self, milliwatts: u32) -> Result<(), Error>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Baseline { pub original_mw: u32, pub requested_mw: u32, pub limits: Limits }
#[derive(Debug, Serialize)]
pub struct TrialReport {
    pub baseline: Baseline,
    pub apply_error: Option<Error>,
    pub applied_readback: Result<u32, Error>,
    pub restore_error: Option<Error>,
    pub restored_readback: Result<u32, Error>,
}
impl TrialReport {
    pub fn success(&self) -> bool {
        self.apply_error.is_none() && self.restore_error.is_none()
            && self.applied_readback == Ok(self.baseline.requested_mw)
            && self.restored_readback == Ok(self.baseline.original_mw)
    }
}

/// Explicit supervised trial only. Persist the original value before the first setter.
/// A setter error can occur after a device change, so restoration is still attempted.
/// The caller retains the persisted receipt for recovery if this process is terminated.
pub fn trial(io: &mut impl PowerIo, target_mw: u32,
    mut persist: impl FnMut(&Baseline) -> Result<(), Error>) -> Result<TrialReport, Error> {
    let limits = io.limits()?;
    let original = io.configured_limit()?;
    if !limits.contains(target_mw) || !limits.contains(original) {
        return Err(Error::local("validate", "Target or original limit is outside the current device range"));
    }
    if original == target_mw {
        return Err(Error::local("validate", "Target equals original; no write/restore trial would be performed"));
    }
    let baseline = Baseline { original_mw: original, requested_mw: target_mw, limits };
    persist(&baseline)?;
    let apply_error = io.set_limit(target_mw).err();
    let applied_readback = io.configured_limit();
    let restore_error = io.set_limit(original).err();
    let restored_readback = io.configured_limit();
    Ok(TrialReport { baseline, apply_error, applied_readback, restore_error, restored_readback })
}

#[cfg(windows)] mod native {
    use super::*;
    use std::{ffi::{c_char, c_void, CString}, ptr};
    use windows::{core::{w, PCSTR}, Win32::{Foundation::{FreeLibrary, HMODULE}, System::LibraryLoader::{GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_SYSTEM32}}};
    type Device = *mut c_void;
    type Init = unsafe extern "C" fn() -> u32;
    type Handle = unsafe extern "C" fn(*const c_char, *mut Device) -> u32;
    type Read = unsafe extern "C" fn(Device, *mut u32) -> u32;
    type Range = unsafe extern "C" fn(Device, *mut u32, *mut u32) -> u32;
    type Set = unsafe extern "C" fn(Device, u32) -> u32;
    struct Library(HMODULE);
    impl Drop for Library { fn drop(&mut self) { unsafe { let _ = FreeLibrary(self.0); } } }
    impl Library {
        fn symbol(&self, name: &'static [u8]) -> Result<unsafe extern "system" fn() -> isize, Error> {
            unsafe { GetProcAddress(self.0, PCSTR(name.as_ptr())) }
                .ok_or_else(|| Error::local("resolve", String::from_utf8_lossy(&name[..name.len()-1])))
        }
    }
    fn check(operation: &str, code: u32) -> Result<(), Error> {
        if code == 0 { return Ok(()); }
        Err(Error { operation: operation.into(), code: Some(code), message: match code {
            3 => "NVML_ERROR_NOT_SUPPORTED", 4 => "NVML_ERROR_NO_PERMISSION",
            9 => "NVML_ERROR_DRIVER_NOT_LOADED", 15 => "NVML_ERROR_GPU_IS_LOST",
            _ => "NVML call failed; original return code preserved",
        }.into() })
    }
    pub struct Nvml {
        _library: Library, shutdown: Init, device: Device,
        read: Read, range: Range, write: Option<Set>,
    }
    impl Nvml {
        /// Explicit native caller only. May wake the GPU during device discovery.
        /// Loading is restricted to System32; no PATH/current-directory DLL lookup.
        pub fn open(uuid: &str, allow_write: bool) -> Result<Self, Error> {
            if !uuid.starts_with("GPU-") { return Err(Error::local("uuid", "An explicit physical GPU UUID is required")); }
            let uuid = CString::new(uuid).map_err(|_| Error::local("uuid", "UUID contains NUL"))?;
            let library = Library(unsafe { LoadLibraryExW(w!("nvml.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32) }
                .map_err(|e| Error::local("load_nvml", e.to_string()))?);
            // Signatures from NVIDIA nvml.h; library remains owned until after shutdown.
            let init: Init = unsafe { std::mem::transmute(library.symbol(b"nvmlInit_v2\0")?) };
            let shutdown: Init = unsafe { std::mem::transmute(library.symbol(b"nvmlShutdown\0")?) };
            let handle: Handle = unsafe { std::mem::transmute(library.symbol(b"nvmlDeviceGetHandleByUUID\0")?) };
            let read: Read = unsafe { std::mem::transmute(library.symbol(b"nvmlDeviceGetPowerManagementLimit\0")?) };
            let range: Range = unsafe { std::mem::transmute(library.symbol(b"nvmlDeviceGetPowerManagementLimitConstraints\0")?) };
            let write = if allow_write { Some(unsafe { std::mem::transmute::<_, Set>(library.symbol(b"nvmlDeviceSetPowerManagementLimit\0")?) }) } else { None };
            check("nvmlInit_v2", unsafe { init() })?;
            let mut backend = Self { _library: library, shutdown, device: ptr::null_mut(), read, range, write };
            check("nvmlDeviceGetHandleByUUID", unsafe { handle(uuid.as_ptr(), &mut backend.device) })?;
            if backend.device.is_null() { return Err(Error::local("device", "NVML returned a null handle")); }
            Ok(backend)
        }
    }
    impl PowerIo for Nvml {
        fn configured_limit(&mut self) -> Result<u32, Error> {
            let mut value = 0;
            check("nvmlDeviceGetPowerManagementLimit", unsafe { (self.read)(self.device, &mut value) })?;
            Ok(value)
        }
        fn limits(&mut self) -> Result<Limits, Error> {
            let (mut min_mw, mut max_mw) = (0, 0);
            check("nvmlDeviceGetPowerManagementLimitConstraints", unsafe { (self.range)(self.device, &mut min_mw, &mut max_mw) })?;
            Ok(Limits { min_mw, max_mw })
        }
        fn set_limit(&mut self, milliwatts: u32) -> Result<(), Error> {
            let write = self.write.ok_or_else(|| Error::local("write", "This NVML session is read only"))?;
            if !self.limits()?.contains(milliwatts) { return Err(Error::local("write", "Limit outside current NVML constraints")); }
            check("nvmlDeviceSetPowerManagementLimit", unsafe { write(self.device, milliwatts) })
        }
    }
    impl Drop for Nvml { fn drop(&mut self) { unsafe { (self.shutdown)(); } } }
}
#[cfg(windows)] pub use native::Nvml;

#[cfg(test)] mod tests {
    use super::*;
    struct Fake { current: u32, writes: Vec<u32>, fail_apply: bool, fail_restore: bool, clamp: bool, fail_apply_read: bool }
    impl Default for Fake { fn default() -> Self { Self { current: 80_000, writes: vec![], fail_apply: false, fail_restore: false, clamp: false, fail_apply_read: false } } }
    impl PowerIo for Fake {
        fn configured_limit(&mut self) -> Result<u32, Error> {
            if self.fail_apply_read && self.writes.len() == 1 { return Err(Error::local("read", "transport lost")); }
            Ok(self.current)
        }
        fn limits(&mut self) -> Result<Limits, Error> { Ok(Limits { min_mw: 50_000, max_mw: 100_000 }) }
        fn set_limit(&mut self, value: u32) -> Result<(), Error> {
            self.writes.push(value);
            if self.fail_restore && value == 80_000 { return Err(Error::local("restore", "lost")); }
            self.current = if self.clamp && value != 80_000 { value - 1_000 } else { value };
            if self.fail_apply && value != 80_000 { return Err(Error::local("set", "failed after mutation")); }
            Ok(())
        }
    }
    #[test] fn exact_apply_and_restore_with_saved_original() {
        let mut io = Fake::default(); let mut receipt = None;
        let report = trial(&mut io, 70_000, |b| { receipt = Some(b.clone()); Ok(()) }).unwrap();
        assert!(report.success()); assert_eq!(io.writes, [70_000, 80_000]);
        assert_eq!(receipt.unwrap().original_mw, 80_000);
    }
    #[test] fn invalid_target_or_failed_persistence_never_writes() {
        let mut io = Fake::default();
        for target in [0, 49_999, 100_001, 80_000] { assert!(trial(&mut io, target, |_| panic!("must not persist")).is_err()); }
        assert!(trial(&mut io, 70_000, |_| Err(Error::local("save", "disk full"))).is_err());
        assert!(io.writes.is_empty());
    }
    #[test] fn setter_failure_after_mutation_still_restores() {
        let mut io = Fake { fail_apply: true, ..Fake::default() };
        let r = trial(&mut io, 70_000, |_| Ok(())).unwrap();
        assert!(!r.success()); assert!(r.apply_error.is_some()); assert_eq!(r.restored_readback, Ok(80_000));
        assert_eq!(io.writes, [70_000, 80_000]);
    }
    #[test] fn clamped_target_is_not_success() {
        let mut io = Fake { clamp: true, ..Fake::default() };
        let r = trial(&mut io, 70_000, |_| Ok(())).unwrap();
        assert!(!r.success()); assert_eq!(r.applied_readback, Ok(69_000)); assert_eq!(io.current, 80_000);
    }
    #[test] fn failed_restore_keeps_original_and_failure_visible() {
        let mut io = Fake { fail_restore: true, ..Fake::default() };
        let r = trial(&mut io, 70_000, |_| Ok(())).unwrap();
        assert!(!r.success()); assert!(r.restore_error.is_some()); assert_eq!(r.baseline.original_mw, 80_000);
        assert_eq!(r.restored_readback, Ok(70_000));
    }
    #[test] fn failed_apply_readback_does_not_skip_restore() {
        let mut io = Fake { fail_apply_read: true, ..Fake::default() };
        let r = trial(&mut io, 70_000, |_| Ok(())).unwrap();
        assert!(!r.success()); assert!(r.applied_readback.is_err());
        assert_eq!(io.writes, [70_000, 80_000]); assert_eq!(r.restored_readback, Ok(80_000));
    }
}
