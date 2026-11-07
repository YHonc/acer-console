//! Read-only discovery. No official getter, mount operation, or target writes.
use serde::{Deserialize,Serialize};
use std::path::PathBuf;

#[derive(Debug,Clone,Serialize)]
pub struct Discovery {
    pub model:String,
    pub bios:String,
    pub esp_volume:String,
    pub esp_oem:PathBuf,
    pub cache:PathBuf,
    pub display_pixels:[u32;2],
    pub maximum_image_pixels:[u32;2],
}

fn valid_volume_name(value:&str)->bool {
    let Some(guid)=value.strip_prefix(r"\\?\Volume{").and_then(|v|v.strip_suffix(r"}\")) else{return false};
    guid.len()==36&&guid.bytes().enumerate().all(|(i,b)|if matches!(i,8|13|18|23){b==b'-'}else{b.is_ascii_hexdigit()})
}

#[cfg(windows)]
pub fn discover()->Result<Discovery,String>{
    std::thread::Builder::new().name("acer-logo-discovery".into()).spawn(discover_windows)
        .map_err(|e|e.to_string())?.join().map_err(|_|"Logo discovery worker stopped".to_string())?
}

#[cfg(not(windows))]
pub fn discover()->Result<Discovery,String>{Err("Boot logo discovery requires Windows".into())}

#[cfg(windows)]
fn discover_windows()->Result<Discovery,String>{
    use wmi::WMIConnection;
    use windows_sys::Win32::{Graphics::Gdi::{DEVMODEW,EnumDisplaySettingsW,ENUM_CURRENT_SETTINGS},UI::Shell::{SHGetKnownFolderPath,FOLDERID_ProgramData},System::Com::CoTaskMemFree};
    use std::{ffi::OsString,os::windows::ffi::OsStringExt};
    #[derive(Deserialize)]struct Computer{#[serde(rename="Model")]model:String}
    #[derive(Deserialize)]struct Bios{#[serde(rename="SMBIOSBIOSVersion")]version:String}
    #[derive(Deserialize)]struct Volume{#[serde(rename="Name")]name:String,#[serde(rename="FileSystem")]filesystem:Option<String>}
    #[derive(Deserialize)]struct Partition{#[serde(rename="AccessPaths")]paths:Option<Vec<String>>,#[serde(rename="GptType")]gpt_type:Option<String>}
    let system=WMIConnection::with_namespace_path("ROOT\\CIMV2").map_err(|e|e.to_string())?;
    let computers:Vec<Computer>=system.raw_query("SELECT Model FROM Win32_ComputerSystem").map_err(|e|e.to_string())?;
    let versions:Vec<Bios>=system.raw_query("SELECT SMBIOSBIOSVersion FROM Win32_BIOS").map_err(|e|e.to_string())?;
    if computers.len()!=1||versions.len()!=1{return Err("Machine identity is ambiguous".into())}
    let model=computers[0].model.clone();let bios=versions[0].version.clone();
    if !matches!(model.as_str(),"PHN16-71"|"Predator PHN16-71")||bios!="V1.18"{return Err(format!("Unsupported boot-logo target: {model} / {bios}"))}
    let volumes:Vec<Volume>=system.raw_query("SELECT Name, FileSystem FROM Win32_Volume WHERE Label='ESP'").map_err(|e|e.to_string())?;
    if volumes.len()!=1{return Err("Exactly one volume labelled ESP is required".into())}
    let volume=&volumes[0];
    if !valid_volume_name(&volume.name)||!volume.filesystem.as_deref().is_some_and(|v|v.eq_ignore_ascii_case("FAT32")){return Err("ESP volume identity or filesystem is not valid".into())}
    let storage=WMIConnection::with_namespace_path("ROOT\\Microsoft\\Windows\\Storage").map_err(|e|e.to_string())?;
    let partitions:Vec<Partition>=storage.raw_query("SELECT AccessPaths, GptType FROM MSFT_Partition").map_err(|e|e.to_string())?;
    let matched:Vec<_>=partitions.iter().filter(|p|p.paths.as_ref().is_some_and(|paths|paths.iter().any(|path|path.eq_ignore_ascii_case(&volume.name)))).collect();
    if matched.len()!=1||!matched[0].gpt_type.as_deref().is_some_and(|s|s.trim_matches(['{','}']).eq_ignore_ascii_case("c12a7328-f81f-11d2-ba4b-00a0c93ec93b")){return Err("The ESP label does not identify a unique EFI system partition".into())}
    let mut raw=std::ptr::null_mut();
    let result=unsafe{SHGetKnownFolderPath(&FOLDERID_ProgramData,0,std::ptr::null_mut(),&mut raw)};
    struct Allocated(*mut u16);impl Drop for Allocated{fn drop(&mut self){unsafe{CoTaskMemFree(self.0.cast())}}}
    let allocated=Allocated(raw);
    if result<0||raw.is_null(){return Err(format!("ProgramData discovery failed: {result:#x}"))}
    let mut length=0;unsafe{while *raw.add(length)!=0{length+=1}}
    let program_data=PathBuf::from(OsString::from_wide(unsafe{std::slice::from_raw_parts(allocated.0,length)}));
    if !program_data.is_absolute(){return Err("ProgramData is not absolute".into())}
    let mut mode:DEVMODEW=unsafe{std::mem::zeroed()};mode.dmSize=std::mem::size_of::<DEVMODEW>() as u16;
    if unsafe{EnumDisplaySettingsW(std::ptr::null(),ENUM_CURRENT_SETTINGS,&mut mode)}==0||mode.dmPelsWidth==0||mode.dmPelsHeight==0{return Err("Current display resolution is unavailable".into())}
    let display_pixels=[mode.dmPelsWidth,mode.dmPelsHeight];
    Ok(Discovery{model,bios,esp_volume:volume.name.clone(),esp_oem:PathBuf::from(&volume.name).join("EFI").join("OEM"),cache:program_data.join("oem").join("AcerAgentService").join("BIOSLogo"),display_pixels,maximum_image_pixels:display_pixels.map(|v|((u64::from(v)*2)/5)as u32)})
}

#[cfg(test)]mod tests{
    use super::*;
    #[test]fn volume_identity_is_a_guid_root_only(){
        assert!(valid_volume_name(r"\\?\Volume{12345678-1234-1234-1234-123456789abc}\"));
        for path in [r"C:\",r"\\server\share\",r"\\?\Volume{12345678-1234-1234-1234-123456789abc}\EFI",r"\\?\Volume{not-a-guid}\"]{assert!(!valid_volume_name(path));}
    }
}
