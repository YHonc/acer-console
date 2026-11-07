//! User-level adapter for the independent Device Center application.
use super::{device_center_module as module, error, SystemError};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

pub(crate) static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[derive(Serialize, Deserialize)]
struct Binding {
    #[serde(default)]
    exe: String,
    #[serde(default)]
    sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    module: Option<module::ModuleBinding>,
    #[serde(flatten)]
    extra: serde_json::Map<String, Value>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceCenterAction {
    Launch,
    EnableStartup,
    DisableStartup,
    Detach,
}

fn config(dir: &Path) -> PathBuf {
    dir.join("device-center-plugin.json")
}

fn load(dir: &Path) -> Result<Option<Binding>, SystemError> {
    match std::fs::read(config(dir)) {
        Ok(bytes) => serde_json::from_slice(&bytes).map(Some).map_err(error),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(error(e)),
    }
}

fn executable(path: &Path) -> Result<(String, String), SystemError> {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if !path.is_absolute()
        || !["DeviceCenter.exe", "device-center.exe", "device_center.exe"]
            .iter()
            .any(|n| name.eq_ignore_ascii_case(n))
    {
        return Err(error("请选择DeviceCenter主程序，不能选择setup安装器"));
    }
    let path = std::fs::canonicalize(path).map_err(error)?;
    let text = module::display_path(&path);
    let mut file = std::fs::File::open(&path).map_err(error)?;
    let size = file.metadata().map_err(error)?.len();
    if !(64..=512 * 1024 * 1024).contains(&size) {
        return Err(error("主程序文件大小异常"));
    }
    let mut magic = [0u8; 2];
    file.read_exact(&mut magic).map_err(error)?;
    if magic != *b"MZ" {
        return Err(error("不是Windows可执行文件"));
    }
    file.seek(SeekFrom::Start(0)).map_err(error)?;
    let mut hash = Sha256::new();
    let mut buf = [0u8; 65536];
    loop {
        let n = file.read(&mut buf).map_err(error)?;
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]);
    }
    Ok((text, format!("{:x}", hash.finalize())))
}

fn ps_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn invoke(exe: &str, action: &str) -> Result<Value, SystemError> {
    let script = format!(
        "$boundExe={}\n$dcAction={}\n{}",
        ps_quote(exe),
        ps_quote(action),
        include_str!("device_center_plugin.ps1")
    );
    super::central_plugin::run_fixed_script(&script)
}

fn append_event(dir: &Path, event: &str) -> Result<(), SystemError> {
    std::fs::create_dir_all(dir).map_err(error)?;
    let path = dir.join("device-center-management.jsonl");
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > 65536) {
        let previous = dir.join("device-center-management.previous.jsonl");
        if previous.exists() {
            std::fs::remove_file(&previous).map_err(error)?;
        }
        std::fs::rename(&path, previous).map_err(error)?;
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(error)?
        .as_millis();
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(error)?;
    writeln!(file, "{}", json!({"timestamp_ms": now, "event": event})).map_err(error)
}

fn validated_entry(
    binding: &Binding,
    module_binding: &module::ModuleBinding,
) -> Result<String, SystemError> {
    let validated = module::validate_binding(module_binding)?;
    let entry = module::display_path(&validated.entry);
    if entry != binding.exe || validated.entry_sha256 != binding.sha256 {
        return Err(error("模块绑定与运行文件不一致，请重新选择模块文件夹"));
    }
    Ok(entry)
}

fn query_inner(dir: &Path) -> Result<Value, SystemError> {
    let binding = load(dir)?;
    let exe = binding.as_ref().map(|b| b.exe.as_str()).unwrap_or("");
    let mut result = invoke(exe, "query")?;
    result["configured"] = json!(binding.is_some());
    result["executable"] = json!(binding.as_ref().map(|b| &b.exe));
    result["binding_valid"] = json!(false);
    result["binding_error"] = Value::Null;
    if let Some(binding) = binding {
        if let Some(module_binding) = binding.module.as_ref() {
            result["module"] = json!({
                "root": module_binding.root,
                "id": module_binding.id,
                "name": module_binding.name,
                "version": module_binding.version
            });
            match validated_entry(&binding, module_binding) {
                Ok(_) => result["binding_valid"] = json!(true),
                Err(e) => result["binding_error"] = json!(e.message),
            }
        } else {
            match executable(Path::new(&binding.exe)) {
                Ok((_, hash)) if hash == binding.sha256 => result["binding_valid"] = json!(true),
                Ok(_) => {
                    result["binding_error"] =
                        json!("程序版本已改变，请关闭本控制台管理的自启动后重新选择主程序")
                }
                Err(e) => result["binding_error"] = json!(e.message),
            }
        }
    }
    Ok(result)
}

pub fn query(dir: &Path) -> Result<Value, SystemError> {
    let _guard = LOCK.lock().map_err(error)?;
    query_inner(dir)
}

pub fn bind(dir: &Path, path: &Path) -> Result<Value, SystemError> {
    let _guard = LOCK.lock().map_err(error)?;
    let prior = query_inner(dir)?;
    if prior["startup"]["present"] == true {
        return Err(error(
            "请先关闭本控制台管理的Device Center自启动，再更换程序路径",
        ));
    }
    let (exe, sha256) = executable(path)?;
    std::fs::create_dir_all(dir).map_err(error)?;
    let pending = dir.join("device-center-plugin.tmp");
    let binding = Binding {
        exe,
        sha256,
        module: None,
        extra: load(dir)?.map(|b| b.extra).unwrap_or_default(),
    };
    std::fs::write(&pending, serde_json::to_vec_pretty(&binding).map_err(error)?).map_err(error)?;
    std::fs::rename(pending, config(dir)).map_err(error)?;
    let log_error = append_event(dir, "binding_saved").err().map(|e| e.message);
    let mut result = query_inner(dir)?;
    result["log_error"] = json!(log_error);
    Ok(result)
}

pub fn bind_module(dir: &Path, module_root: &Path) -> Result<Value, SystemError> {
    let _guard = LOCK.lock().map_err(error)?;
    let prior = query_inner(dir)?;
    if prior["startup"]["present"] == true {
        return Err(error(
            "请先关闭本控制台管理的Device Center自启动，再更换模块文件夹",
        ));
    }
    let validated = module::bind_root(module_root)?;
    let entry = module::display_path(&validated.entry);
    std::fs::create_dir_all(dir).map_err(error)?;
    let pending = dir.join("device-center-plugin.tmp");
    let binding = Binding {
        exe: entry,
        sha256: validated.entry_sha256,
        module: Some(validated.binding),
        extra: load(dir)?.map(|b| b.extra).unwrap_or_default(),
    };
    std::fs::write(&pending, serde_json::to_vec_pretty(&binding).map_err(error)?).map_err(error)?;
    std::fs::rename(pending, config(dir)).map_err(error)?;
    let log_error = append_event(dir, "module_bound").err().map(|e| e.message);
    let mut result = query_inner(dir)?;
    result["log_error"] = json!(log_error);
    Ok(result)
}

fn detach_blocker(current: &Value) -> Option<&'static str> {
    if current["startup"]["present"] == true {
        Some("本控制台仍管理Device Center自启动，请先关闭自启动再解除绑定")
    } else if current["running"] == true
        || current["processes"]
            .as_array()
            .is_some_and(|processes| !processes.is_empty())
    {
        Some("Device Center仍在运行，请先退出原应用再解除绑定")
    } else {
        None
    }
}

fn detach(dir: &Path, binding: Option<Binding>) -> Result<Value, SystemError> {
    if binding.is_none() {
        return Err(error("尚未绑定Device Center模块"));
    }
    let current = query_inner(dir)?;
    if let Some(message) = detach_blocker(&current) {
        return Err(error(message));
    }
    std::fs::remove_file(config(dir)).map_err(error)?;
    let log_error = append_event(dir, "detached").err().map(|e| e.message);
    let mut result = query_inner(dir)?;
    result["message"] = json!("本控制台绑定已解除；程序与账号数据保留。");
    result["log_error"] = json!(log_error);
    Ok(result)
}

pub fn action(dir: &Path, action: DeviceCenterAction) -> Result<Value, SystemError> {
    let _guard = LOCK.lock().map_err(error)?;
    let binding = load(dir)?;
    if matches!(action, DeviceCenterAction::Detach) {
        return detach(dir, binding);
    }
    let exe = match binding.as_ref() {
        Some(binding) if !matches!(action, DeviceCenterAction::DisableStartup) => {
            match binding.module.as_ref() {
                Some(module_binding) => validated_entry(binding, module_binding)?,
                None => {
                    if executable(Path::new(&binding.exe))?.1 != binding.sha256 {
                        return Err(error("程序哈希发生变化，请重新绑定"));
                    }
                    binding.exe.clone()
                }
            }
        }
        Some(binding) => binding.exe.clone(),
        None if matches!(action, DeviceCenterAction::DisableStartup) => String::new(),
        None => return Err(error("请先选择DeviceCenter主程序")),
    };
    let name = match action {
        DeviceCenterAction::Launch => "launch",
        DeviceCenterAction::EnableStartup => "enable_startup",
        DeviceCenterAction::DisableStartup => "disable_startup",
        DeviceCenterAction::Detach => unreachable!(),
    };
    let response = invoke(&exe, name);
    let log_error = append_event(dir, if response.is_ok() { name } else { "action_failed" })
        .err()
        .map(|e| e.message);
    let mut result = response?;
    result["log_error"] = json!(log_error);
    Ok(result)
}

pub fn log(dir: &Path) -> Result<Value, SystemError> {
    let _guard = LOCK.lock().map_err(error)?;
    let path = dir.join("device-center-management.jsonl");
    let mut file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(json!({"text": "尚无管理操作记录", "truncated": false}))
        }
        Err(e) => return Err(error(e)),
    };
    let size = file.metadata().map_err(error)?.len();
    let offset = size.saturating_sub(32768);
    file.seek(SeekFrom::Start(offset)).map_err(error)?;
    let mut bytes = Vec::new();
    file.take(32768).read_to_end(&mut bytes).map_err(error)?;
    if offset > 0 {
        if let Some(i) = bytes.iter().position(|x| *x == b'\n') {
            bytes.drain(..=i);
        }
    }
    Ok(json!({"text": String::from_utf8_lossy(&bytes), "truncated": offset > 0}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closed_actions_keep_only_supported_actions() {
        assert!(serde_json::from_value::<DeviceCenterAction>(json!("launch")).is_ok());
        assert!(serde_json::from_value::<DeviceCenterAction>(json!("detach")).is_ok());
        for value in ["kill", "restart", "invoke", "delete"] {
            assert!(serde_json::from_value::<DeviceCenterAction>(json!(value)).is_err());
        }
    }

    #[test]
    fn old_binding_without_module_still_loads() {
        let binding: Binding =
            serde_json::from_str(r#"{"exe":"C:\\DeviceCenter.exe","sha256":"abc"}"#).unwrap();
        assert_eq!(binding.exe, r"C:\DeviceCenter.exe");
        assert!(binding.module.is_none());
    }

    #[test]
    fn ipc_only_and_extended_legacy_configs_keep_new_fields() {
        for value in [json!({"keep":true,"trustSnapshot":{"files":[]}}),json!({"exe":"old.exe","sha256":"abc","keep":true,"trustSnapshot":{"files":[]}})] {
            let binding:Binding=serde_json::from_value(value.clone()).unwrap();
            let saved=serde_json::to_value(binding).unwrap();
            assert_eq!(saved["keep"],true);assert_eq!(saved["trustSnapshot"],value["trustSnapshot"]);
        }
    }

    #[test]
    fn detach_is_blocked_by_startup_or_running_process() {
        assert!(detach_blocker(&json!({
            "startup": {"present": true},
            "running": false,
            "processes": []
        }))
        .is_some());
        assert!(detach_blocker(&json!({
            "startup": {"present": false},
            "running": true,
            "processes": []
        }))
        .is_some());
        assert!(detach_blocker(&json!({
            "startup": {"present": false},
            "running": false,
            "processes": [{"pid": 1}]
        }))
        .is_some());
        assert!(detach_blocker(&json!({
            "startup": {"present": false},
            "running": false,
            "processes": []
        }))
        .is_none());
    }

    #[test]
    fn paths_are_data_not_script() {
        assert_eq!(
            ps_quote("C:\\x' ; Stop-Process x"),
            "'C:\\x'' ; Stop-Process x'"
        );
    }

    #[test]
    fn installers_and_relative_paths_are_rejected() {
        assert!(executable(Path::new("DeviceCenter.exe")).is_err());
        assert!(executable(Path::new(r"C:\DeviceCenter-setup.exe")).is_err());
    }
}
