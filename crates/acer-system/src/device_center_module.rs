//! Validation for the Device Center folder-module manifest.
use crate::{error, SystemError};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs::{self, File},
    io::Read,
    path::{Component, Path, PathBuf},
};

const MANIFEST_FILE: &str = "module.json";
const MODULE_ID: &str = "device-center";
const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
const MAX_ENTRY_BYTES: u64 = 512 * 1024 * 1024;
const ENTRY_NAMES: [&str; 3] = ["DeviceCenter.exe", "device-center.exe", "device_center.exe"];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct ModuleFile {
    pub path: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ModuleBinding {
    pub root: String,
    pub manifest_sha256: String,
    pub id: String,
    pub name: String,
    pub version: String,
    pub entry: String,
    pub files: Vec<ModuleFile>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema_version: u32,
    id: String,
    name: String,
    version: String,
    entry: String,
    files: Vec<ModuleFile>,
}

pub(crate) struct ValidatedModule {
    pub binding: ModuleBinding,
    pub entry: PathBuf,
    pub entry_sha256: String,
}

fn fail(message: impl Into<String>) -> SystemError {
    error(message.into())
}

fn sha256_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn sha256_file(path: &Path) -> Result<String, SystemError> {
    let mut file = File::open(path).map_err(error)?;
    let mut hash = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let count = file.read(&mut buf).map_err(error)?;
        if count == 0 {
            break;
        }
        hash.update(&buf[..count]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn is_reparse(meta: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        return meta.file_attributes() & 0x400 != 0;
    }
    #[cfg(not(windows))]
    {
        meta.file_type().is_symlink()
    }
}

fn canonical_root(root: &Path) -> Result<PathBuf, SystemError> {
    let meta = fs::symlink_metadata(root).map_err(error)?;
    if !meta.is_dir() || is_reparse(&meta) {
        return Err(fail("模块根目录不是普通文件夹"));
    }
    let canonical = fs::canonicalize(root).map_err(error)?;
    let canonical_meta = fs::symlink_metadata(&canonical).map_err(error)?;
    if !canonical_meta.is_dir() || is_reparse(&canonical_meta) {
        return Err(fail("模块根目录重定向被拒绝"));
    }
    Ok(canonical)
}

fn relative_path(raw: &str) -> Result<PathBuf, SystemError> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(fail("模块清单包含空文件路径"));
    }
    let path = Path::new(raw);
    if path.is_absolute() {
        return Err(fail("模块清单不允许绝对路径"));
    }
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) if !part.is_empty() => result.push(part),
            _ => return Err(fail("模块清单不允许上级、根或重定向路径")),
        }
    }
    if result.as_os_str().is_empty() {
        return Err(fail("模块清单文件路径为空"));
    }
    Ok(result)
}

fn normalized_path(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "/")
        .to_ascii_lowercase()
}

fn checked_file(root: &Path, relative: &Path) -> Result<PathBuf, SystemError> {
    let mut current = root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(part) = component else {
            return Err(fail("模块文件路径越界"));
        };
        current.push(part);
        let meta = fs::symlink_metadata(&current).map_err(error)?;
        if is_reparse(&meta) {
            return Err(fail("模块文件重定向被拒绝"));
        }
    }
    let canonical = fs::canonicalize(&current).map_err(error)?;
    if !canonical.starts_with(root) {
        return Err(fail("模块文件路径越界"));
    }
    let meta = fs::metadata(&canonical).map_err(error)?;
    if !meta.is_file() {
        return Err(fail("模块运行文件不是普通文件"));
    }
    Ok(canonical)
}

fn validate_hash(value: &str) -> Result<String, SystemError> {
    let value = value.trim().to_ascii_lowercase();
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(fail("模块清单 SHA256 格式无效"));
    }
    Ok(value)
}

fn validate_text(label: &str, value: &str, max: usize) -> Result<(), SystemError> {
    if value.trim().is_empty() || value.len() > max || value.chars().any(char::is_control) {
        return Err(fail(format!("module.json 的 {label} 无效")));
    }
    Ok(())
}

fn entry_name(entry: &str) -> Result<(), SystemError> {
    let path = Path::new(entry);
    let mut components = path.components();
    let Some(Component::Normal(name)) = components.next() else {
        return Err(fail("module.json 的 entry 必须是文件名"));
    };
    if components.next().is_some() {
        return Err(fail("module.json 的 entry 不允许目录或上级路径"));
    }
    let name = name.to_string_lossy();
    if !ENTRY_NAMES.iter().any(|allowed| allowed.eq_ignore_ascii_case(&name)) {
        return Err(fail("module.json 的 entry 不是受支持的 Device Center 主程序"));
    }
    Ok(())
}

fn parse_manifest(bytes: &[u8]) -> Result<Manifest, SystemError> {
    let manifest: Manifest =
        serde_json::from_slice(bytes).map_err(|_| fail("module.json 格式无效"))?;
    if manifest.schema_version != 1 || manifest.id != MODULE_ID {
        return Err(fail("不接受此外部模块的 schema_version 或 id"));
    }
    validate_text("name", &manifest.name, 128)?;
    validate_text("version", &manifest.version, 64)?;
    entry_name(&manifest.entry)?;
    if manifest.files.is_empty() {
        return Err(fail("module.json 没有 runtime files"));
    }
    Ok(manifest)
}

fn validate_files(
    root: &Path,
    manifest: &Manifest,
) -> Result<(Vec<ModuleFile>, String), SystemError> {
    let mut seen = HashSet::new();
    let mut files = Vec::with_capacity(manifest.files.len());
    for file in &manifest.files {
        let relative = relative_path(&file.path)?;
        let key = normalized_path(&relative);
        if !seen.insert(key) {
            return Err(fail("module.json 的 runtime files 存在重复路径"));
        }
        let hash = validate_hash(&file.sha256)?;
        let path = checked_file(root, &relative)?;
        if sha256_file(&path)? != hash {
            return Err(fail(format!("模块文件哈希不一致：{}", file.path)));
        }
        files.push(ModuleFile {
            path: relative.to_string_lossy().into_owned(),
            sha256: hash,
        });
    }

    let entry_relative = Path::new(&manifest.entry);
    let entry_key = normalized_path(entry_relative);
    if !seen.contains(&entry_key) {
        return Err(fail("module.json 的 runtime files 未包含 entry"));
    }
    let entry_path = checked_file(root, entry_relative)?;
    let entry_sha256 = sha256_file(&entry_path)?;
    let size = fs::metadata(&entry_path).map_err(error)?.len();
    if !(64..=MAX_ENTRY_BYTES).contains(&size) {
        return Err(fail("模块主程序大小异常"));
    }
    let mut magic = [0u8; 2];
    File::open(&entry_path)
        .map_err(error)?
        .read_exact(&mut magic)
        .map_err(error)?;
    if magic != *b"MZ" {
        return Err(fail("模块 entry 不是 Windows 可执行文件"));
    }
    Ok((files, entry_sha256))
}

fn read_manifest(root: &Path) -> Result<(Vec<u8>, Manifest), SystemError> {
    let path = checked_file(root, Path::new(MANIFEST_FILE))?;
    let size = fs::metadata(&path).map_err(error)?.len();
    if size > MAX_MANIFEST_BYTES {
        return Err(fail("module.json 过大"));
    }
    let bytes = fs::read(&path).map_err(error)?;
    let manifest = parse_manifest(&bytes)?;
    Ok((bytes, manifest))
}

pub(crate) fn bind_root(root: &Path) -> Result<ValidatedModule, SystemError> {
    let root = canonical_root(root)?;
    let (bytes, manifest) = read_manifest(&root)?;
    let (files, entry_sha256) = validate_files(&root, &manifest)?;
    let entry = checked_file(&root, Path::new(&manifest.entry))?;
    Ok(ValidatedModule {
        binding: ModuleBinding {
            root: display_path(&root),
            manifest_sha256: sha256_bytes(&bytes),
            id: manifest.id,
            name: manifest.name,
            version: manifest.version,
            entry: manifest.entry,
            files,
        },
        entry,
        entry_sha256,
    })
}

pub(crate) fn validate_binding(binding: &ModuleBinding) -> Result<ValidatedModule, SystemError> {
    let root = canonical_root(Path::new(&binding.root))?;
    if display_path(&root) != binding.root {
        return Err(fail("模块根目录已改变，请重新绑定"));
    }
    let (bytes, manifest) = read_manifest(&root)?;
    if sha256_bytes(&bytes) != binding.manifest_sha256 {
        return Err(fail("module.json 已改变，请重新绑定"));
    }
    if manifest.id != binding.id
        || manifest.name != binding.name
        || manifest.version != binding.version
        || manifest.entry != binding.entry
    {
        return Err(fail("模块绑定与 module.json 不一致，请重新绑定"));
    }
    let (files, entry_sha256) = validate_files(&root, &manifest)?;
    if files != binding.files {
        return Err(fail("模块运行文件清单与绑定不一致，请重新绑定"));
    }
    let entry = checked_file(&root, Path::new(&manifest.entry))?;
    Ok(ValidatedModule {
        binding: binding.clone(),
        entry,
        entry_sha256,
    })
}

pub(crate) fn display_path(path: &Path) -> String {
    path.to_string_lossy()
        .trim_start_matches(r"\\?\")
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "acer-dc-module-{}-{}-{}",
            std::process::id(),
            label,
            nonce
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn write_module(root: &Path, exe: &[u8], extra: &[u8], extra_name: &str) -> String {
        fs::write(root.join("DeviceCenter.exe"), exe).unwrap();
        fs::write(root.join(extra_name), extra).unwrap();
        let manifest = format!(
            r#"{{"schema_version":1,"id":"device-center","name":"Device Center","version":"0.1.1","entry":"DeviceCenter.exe","files":[{{"path":"DeviceCenter.exe","sha256":"{}"}},{{"path":"{}","sha256":"{}"}}]}}"#,
            sha256_bytes(exe),
            extra_name,
            sha256_bytes(extra)
        );
        fs::write(root.join(MANIFEST_FILE), manifest).unwrap();
        sha256_bytes(fs::read(root.join(MANIFEST_FILE)).unwrap().as_slice())
    }

    #[test]
    fn valid_manifest_binds_and_revalidates() {
        let root = temp_dir("valid");
        let mut exe = vec![0u8; 64];
        exe[..2].copy_from_slice(b"MZ");
        write_module(&root, &exe, b"loader", "WebView2Loader.dll");
        let first = bind_root(&root).unwrap();
        assert_eq!(first.binding.id, "device-center");
        assert_eq!(first.binding.entry, "DeviceCenter.exe");
        assert!(validate_binding(&first.binding).is_ok());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn changed_runtime_file_invalidates_binding() {
        let root = temp_dir("tampered-file");
        let mut exe = vec![0u8; 64];
        exe[..2].copy_from_slice(b"MZ");
        write_module(&root, &exe, b"loader", "WebView2Loader.dll");
        let first = bind_root(&root).unwrap();
        fs::write(root.join("WebView2Loader.dll"), b"changed").unwrap();
        assert!(validate_binding(&first.binding).is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn changed_manifest_invalidates_binding() {
        let root = temp_dir("tampered-manifest");
        let mut exe = vec![0u8; 64];
        exe[..2].copy_from_slice(b"MZ");
        write_module(&root, &exe, b"loader", "WebView2Loader.dll");
        let first = bind_root(&root).unwrap();
        fs::write(root.join(MANIFEST_FILE), b"{}").unwrap();
        assert!(validate_binding(&first.binding).is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn traversal_absolute_and_unsupported_entry_are_rejected() {
        let root = temp_dir("paths");
        let mut exe = vec![0u8; 64];
        exe[..2].copy_from_slice(b"MZ");
        write_module(&root, &exe, b"loader", "WebView2Loader.dll");
        let manifest_path = root.join(MANIFEST_FILE);
        let valid = fs::read_to_string(&manifest_path).unwrap();
        let traversal = valid.replace("WebView2Loader.dll", "../escape.dll");
        fs::write(&manifest_path, traversal).unwrap();
        assert!(bind_root(&root).is_err());

        fs::write(&manifest_path, valid.replace("WebView2Loader.dll", "C:/escape.dll")).unwrap();
        assert!(bind_root(&root).is_err());

        fs::write(&manifest_path, valid.replace("DeviceCenter.exe", "setup.exe")).unwrap();
        assert!(bind_root(&root).is_err());
        let _ = fs::remove_dir_all(root);
    }
}
