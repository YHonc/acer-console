//! Explicit system integration; loading settings never modifies startup state.
use serde::{Deserialize, Serialize};
use std::path::Path;
pub mod central_plugin;
pub mod device_center_plugin;
mod device_center_module;
pub mod device_center_ipc;
pub mod device_center_runtime;
pub mod module_discovery;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemError { pub message: String }
impl std::fmt::Display for SystemError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str(&self.message) }
}
impl std::error::Error for SystemError {}
fn error(e: impl ToString) -> SystemError { SystemError { message: e.to_string() } }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartupState { pub enabled: bool, pub entry_present: bool, pub mismatch: bool }

pub fn get_startup() -> Result<StartupState, SystemError> {
    #[cfg(windows)] { startup::get() }
    #[cfg(not(windows))] { Err(error("Startup registration requires Windows")) }
}
pub fn startup_enabled() -> Result<bool, SystemError> { Ok(get_startup()?.enabled) }
/// Explicit user action only; never call as a side effect of reading settings.
pub fn set_startup(enabled: bool) -> Result<StartupState, SystemError> {
    #[cfg(windows)] { startup::set(enabled) }
    #[cfg(not(windows))] { let _ = enabled; Err(error("Startup registration requires Windows")) }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all="lowercase")]
pub enum Theme { #[default] System, Light, Dark }
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub theme: Theme,
    pub poll_interval_ms: u32,
    pub close_to_tray: bool,
    pub restore_on_exit: bool,
}
impl Default for Settings {
    fn default() -> Self { Self { theme: Theme::System, poll_interval_ms: 2000, close_to_tray: false, restore_on_exit: false } }
}
impl Settings {
    pub fn validate(&self) -> Result<(), SystemError> {
        if !(1000..=60000).contains(&self.poll_interval_ms) { return Err(error("poll_interval_ms must be 1000..=60000")); }
        Ok(())
    }
}
pub fn read_settings(config_dir: &Path) -> Result<Settings, SystemError> {
    let bytes = match std::fs::read(config_dir.join("settings.json")) {
        Ok(v) => v,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Settings::default()),
        Err(e) => return Err(error(e)),
    };
    let settings: Settings = serde_json::from_slice(&bytes).map_err(error)?;
    settings.validate()?; Ok(settings)
}
pub fn write_settings(config_dir: &Path, settings: &Settings) -> Result<Settings, SystemError> {
    use std::io::Write;
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    static SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    settings.validate()?;
    let bytes = serde_json::to_vec_pretty(settings).map_err(error)?;
    let _guard = LOCK.lock().map_err(error)?;
    std::fs::create_dir_all(config_dir).map_err(error)?;
    let sequence = SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let temp = config_dir.join(format!("settings-{}-{sequence}.tmp", std::process::id()));
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&temp).map_err(error)?;
    let result = (|| {
        file.write_all(&bytes).map_err(error)?;
        file.sync_all().map_err(error)?;
        drop(file);
        std::fs::rename(&temp, config_dir.join("settings.json")).map_err(error)
    })();
    if result.is_err() { let _ = std::fs::remove_file(&temp); }
    result?; Ok(settings.clone())
}
#[cfg(windows)] mod startup;
mod official_task;
pub mod power_watch;
pub mod sticky_keys;
pub mod calibration_power;
pub mod keep_awake;
pub mod directory_guard;
pub use official_task::{backup_official_task, disable_official_task, get_official_task, restore_official_task, get_official_backup, OfficialBackupState, OfficialTaskState};

#[cfg(test)] mod tests {
    use super::*;
    #[test] fn config_defaults_and_validation() {
        let mut settings = Settings::default(); assert!(settings.validate().is_ok());
        for value in [0,999,60001,u32::MAX] { settings.poll_interval_ms=value; assert!(settings.validate().is_err()); }
        assert!(serde_json::from_str::<Settings>(r#"{"theme":"unknown","poll_interval_ms":2000,"close_to_tray":false,"restore_on_exit":false}"#).is_err());
    }
    #[test] fn isolated_settings_roundtrip_and_replace() {
        let dir = std::env::temp_dir().join(format!("acer-system-test-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        assert_eq!(read_settings(&dir).unwrap(), Settings::default());
        let mut settings = Settings::default(); write_settings(&dir,&settings).unwrap();
        settings.theme=Theme::Dark; write_settings(&dir,&settings).unwrap();
        assert_eq!(read_settings(&dir).unwrap(),settings);
        std::fs::write(dir.join("settings.json"),b"broken").unwrap();
        assert!(read_settings(&dir).is_err()); std::fs::remove_dir_all(&dir).unwrap();
    }
}
