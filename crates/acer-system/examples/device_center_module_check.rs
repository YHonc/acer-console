use acer_system::device_center_plugin::{self as dc, DeviceCenterAction};
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

fn unique_temp_config() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "acer-dc-module-check-{}-{}",
        std::process::id(),
        nonce
    ))
}

fn module_files_exist(root: &Path) -> Result<bool, String> {
    let bytes = fs::read(root.join("module.json")).map_err(|e| format!("读取模块清单失败：{e}"))?;
    let manifest: Value =
        serde_json::from_slice(&bytes).map_err(|e| format!("模块清单格式失败：{e}"))?;
    let files = manifest["files"]
        .as_array()
        .ok_or_else(|| "模块清单没有 files".to_owned())?;
    Ok(files.iter().all(|file| {
        file["path"]
            .as_str()
            .is_some_and(|path| root.join(path).is_file())
    }))
}

fn run(module_root: &Path) -> Result<Value, String> {
    let config_dir = unique_temp_config();
    fs::create_dir_all(&config_dir).map_err(|e| format!("创建临时配置目录失败：{e}"))?;

    let bound = dc::bind_module(&config_dir, module_root)
        .map_err(|e| format!("bind_module：{}", e.message))?;
    if bound["configured"].as_bool() != Some(true)
        || bound["binding_valid"].as_bool() != Some(true)
    {
        return Err(format!("bind_module 返回状态不符合预期：{bound}"));
    }
    let first = dc::query(&config_dir).map_err(|e| format!("query after bind：{}", e.message))?;
    let module = first
        .get("module")
        .cloned()
        .ok_or_else(|| "query 未返回 module".to_owned())?;
    let configured = first["configured"].as_bool() == Some(true);
    let binding_valid = first["binding_valid"].as_bool() == Some(true);
    let version = module["version"].as_str().unwrap_or("");
    if !configured || !binding_valid || module["id"] != "device-center" || version.is_empty() {
        return Err(format!(
            "绑定后的查询不符合预期：configured={configured} binding_valid={binding_valid} module={module}"
        ));
    }

    let detached = match dc::action(&config_dir, DeviceCenterAction::Detach) {
        Ok(value) => value,
        Err(e) => {
            let current = dc::query(&config_dir)
                .map_err(|query_error| format!("detach 被阻挡，且查询失败：{}", query_error.message))?;
            return Err(format!(
                "detach 被阻挡，未绕过：{}；当前 running={} startup.present={}",
                e.message,
                current["running"],
                current["startup"]["present"]
            ));
        }
    };
    let after = dc::query(&config_dir).map_err(|e| format!("query after detach：{}", e.message))?;
    let binding_file = config_dir.join("device-center-plugin.json");
    let files_exist = module_files_exist(module_root)?;
    let module_still_exists = module_root.is_dir();
    let remaining_module = after.get("module");
    let ok = after["configured"].as_bool() == Some(false)
        && after["binding_valid"].as_bool() == Some(false)
        && (remaining_module.is_none() || remaining_module == Some(&Value::Null))
        && !binding_file.exists()
        && files_exist
        && module_still_exists;
    if !ok {
        return Err(format!(
            "detach 后状态不符合预期：configured={} binding_valid={} module={:?} binding_file_exists={} module_files_exist={} module_still_exists={}",
            after["configured"],
            after["binding_valid"],
            remaining_module,
            binding_file.exists(),
            files_exist,
            module_still_exists
        ));
    }

    let result = json!({
        "status": "ok",
        "module_root": module_root.to_string_lossy(),
        "module": module,
        "bound": {
            "configured": configured,
            "binding_valid": binding_valid
        },
        "detach": {
            "configured": after["configured"],
            "binding_valid": after["binding_valid"],
            "binding_file_exists": binding_file.exists(),
            "module_files_still_exist": files_exist,
            "module_root_still_exists": module_still_exists,
            "message": detached["message"]
        },
        "scope": "isolated temporary config; no EXE launch or real startup write; query-only CIM/Run reads",
        "temp_config": "<isolated temporary directory>"
    });
    let _ = fs::remove_dir_all(&config_dir);
    Ok(result)
}

fn main() {
    let Some(module_root) = std::env::args().nth(1).map(PathBuf::from) else {
        println!(
            "{}",
            json!({"status":"error","error":"usage: device_center_module_check <module-root>"})
        );
        std::process::exit(2);
    };
    match run(&module_root) {
        Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
        Err(error) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "status": "error",
                    "module_root": module_root.to_string_lossy(),
                    "error": error,
                    "scope": "isolated temporary config; no EXE launch or real startup write; query-only CIM/Run reads"
                }))
                .unwrap()
            );
            std::process::exit(1);
        }
    }
}
