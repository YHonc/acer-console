//! Separate native diagnostic. No invocation without an exact mode and GPU UUID.
use acer_device::gpu_power::{Baseline, Error};
use serde::{Deserialize, Serialize};
use std::{fs::{File, OpenOptions}, io::Write, path::PathBuf};
const USAGE: &str = "Usage:\n  gpu-power --read GPU-UUID\n  gpu-power --trial GPU-UUID MILLIWATTS --restore RECEIPT.json\n  gpu-power --restore-receipt GPU-UUID RECEIPT.json\n\nNo args/--help do not load NVML. Read can wake the GPU. Trial and restore change hardware and require a separately arranged supervised test. No elevation or driver installation is performed. Values are integer milliwatts, not a Dynamic Boost/TGP guarantee.";
#[derive(Debug, PartialEq)]
enum Mode { Read(String), Trial(String, u32, PathBuf), Restore(String, PathBuf) }
fn parse(a: &[String]) -> Option<Mode> {
    match a {
        [flag, uuid] if flag == "--read" && uuid.starts_with("GPU-") => Some(Mode::Read(uuid.clone())),
        [flag, uuid, value, restore, path] if flag == "--trial" && uuid.starts_with("GPU-") && restore == "--restore" =>
            value.parse::<u32>().ok().filter(|v| *v > 0).map(|v| Mode::Trial(uuid.clone(), v, path.into())),
        [flag, uuid, path] if flag == "--restore-receipt" && uuid.starts_with("GPU-") => Some(Mode::Restore(uuid.clone(), path.into())),
        _ => None,
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt { version: u32, uuid: String, captured_at_ms: u128, baseline: Baseline }
fn persist(path: &PathBuf, receipt: &Receipt) -> Result<(), Error> {
    // Never overwrite the only recovery record from a prior session.
    let bytes = serde_json::to_vec_pretty(receipt).map_err(|e| Error::local("receipt_json", e.to_string()))?;
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)
        .map_err(|e| Error::local("create_receipt", e.to_string()))?;
    file.write_all(&bytes).and_then(|_| file.sync_all()).map_err(|e| Error::local("flush_receipt", e.to_string()))
}
#[cfg(windows)]
fn run(mode: Mode) -> Result<(serde_json::Value, bool), Error> {
    use acer_device::gpu_power::{trial, Nvml, PowerIo};
    match mode {
        Mode::Read(uuid) => {
            let mut io = Nvml::open(&uuid, false)?;
            let limit = io.configured_limit();
            let constraints = io.limits();
            let success = limit.is_ok();
            Ok((serde_json::json!({"uuid":uuid,"configured_limit_mw":limit,"constraints":constraints,
                "write_support":"unverified","source":"NVML configured limit; not measured power or enforced Dynamic Boost limit"}), success))
        }
        Mode::Trial(uuid, target, path) => {
            let mut io = Nvml::open(&uuid, true)?;
            let report = trial(&mut io, target, |baseline| persist(&path, &Receipt {
                version: 1, uuid: uuid.clone(), baseline: baseline.clone(),
                captured_at_ms: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                    .map_err(|e| Error::local("clock", e.to_string()))?.as_millis(),
            }))?;
            let success = report.success();
            Ok((serde_json::json!({"uuid":uuid,"receipt":path,"report":report,"success":success}), success))
        }
        Mode::Restore(uuid, path) => {
            let file = File::open(&path).map_err(|e| Error::local("read_receipt", e.to_string()))?;
            let receipt: Receipt = serde_json::from_reader(file).map_err(|e| Error::local("receipt_json", e.to_string()))?;
            if receipt.version != 1 || receipt.uuid != uuid { return Err(Error::local("restore", "Receipt version or GPU UUID mismatch")); }
            let mut io = Nvml::open(&uuid, true)?;
            let before = io.configured_limit()?;
            let target = receipt.baseline.original_mw;
            let write_error = if before == target { None } else { io.set_limit(target).err() };
            let after = io.configured_limit();
            let success = write_error.is_none() && after == Ok(target);
            Ok((serde_json::json!({"uuid":uuid,"receipt":path,"before_mw":before,"original_mw":target,
                "write_error":write_error,"restored_readback":after,"success":success}), success))
        }
    }
}
#[cfg(not(windows))]
fn run(_: Mode) -> Result<(serde_json::Value, bool), Error> { Err(Error::local("platform", "Windows NVML backend required")) }
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if a.is_empty() || a == ["--help"] { println!("{USAGE}"); return; }
    let Some(mode) = parse(&a) else { eprintln!("{USAGE}"); std::process::exit(2); };
    match run(mode) {
        Ok((report, success)) => { println!("{}", serde_json::to_string_pretty(&report).unwrap()); if !success { std::process::exit(1); } }
        Err(e) => { eprintln!("{}", serde_json::to_string_pretty(&e).unwrap()); std::process::exit(1); }
    }
}
#[cfg(test)] mod tests {
    use super::*;
    fn args(s: &str) -> Vec<String> { s.split_whitespace().map(str::to_owned).collect() }
    #[test] fn writes_require_explicit_trial_restore_and_integer_milliwatts() {
        for s in ["", "--trial GPU-test 70000", "--trial GPU-test 70.5 --restore r.json", "--trial GPU-test 0 --restore r.json", "--trial 0 70000 --restore r.json", "--write 70000"] {
            assert_eq!(parse(&args(s)), None);
        }
        assert!(matches!(parse(&args("--trial GPU-test 70000 --restore r.json")), Some(Mode::Trial(_, 70000, _))));
        assert!(matches!(parse(&args("--read GPU-test")), Some(Mode::Read(_))));
    }
    #[test] fn existing_receipt_is_never_overwritten() {
        let path = std::env::temp_dir().join(format!("acer-gpu-receipt-test-{}-{}.json", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let receipt = Receipt { version: 1, uuid: "GPU-test".into(), captured_at_ms: 1,
            baseline: Baseline { original_mw: 80_000, requested_mw: 70_000, limits: acer_device::gpu_power::Limits { min_mw: 50_000, max_mw: 100_000 } } };
        persist(&path, &receipt).unwrap();
        let original = std::fs::read(&path).unwrap();
        assert!(persist(&path, &receipt).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), original);
        std::fs::remove_file(path).unwrap();
    }
}
