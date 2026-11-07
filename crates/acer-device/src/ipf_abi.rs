//! Packed IPF SDK ABI for Windows x64. No loading or hardware calls in this module.
//! Layout source: intel/dptf@468699545442788999071d0a8ccb71b35332cdf1,
//! Common/esif_sdk_iface_{app,esif}.h and IPF/Sources/Common/ipf_{ipc_iface,core_api}.h.
//! Read packed members by value or read_unaligned; never borrow their fields.
#![allow(dead_code)]
use std::ffi::c_void;

pub type Status = i32;
pub type Handle = u64;
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct Header { pub kind: u32, pub version: u16, pub size: u16 }
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct Data { pub kind: u32, pub buffer: *mut c_void, pub capacity: u32, pub length: u32 }
#[repr(C, packed)]
pub struct AppData { pub path: Data, pub log_level: i32 }

pub type GetText = unsafe extern "system" fn(*mut Data) -> Status;
pub type Create = unsafe extern "system" fn(*mut AppInterface, Handle, *mut Handle, *const AppData, i32) -> Status;
pub type AppAction = unsafe extern "system" fn(Handle) -> Status;
pub type Intro = unsafe extern "system" fn(Handle, *mut Data) -> Status;
pub type Command = unsafe extern "system" fn(Handle, u32, *const Data, *mut Data) -> Status;
pub type GetStatus = unsafe extern "system" fn(Handle, i32, u32, *mut Data) -> Status;
pub type ParticipantCreate = unsafe extern "system" fn(Handle, Handle, *const ParticipantData, i32) -> Status;
pub type ParticipantDestroy = unsafe extern "system" fn(Handle, Handle) -> Status;
pub type DomainCreate = unsafe extern "system" fn(Handle, Handle, Handle, *const DomainData, i32) -> Status;
pub type DomainDestroy = unsafe extern "system" fn(Handle, Handle, Handle) -> Status;
pub type Event = unsafe extern "system" fn(Handle, Handle, Handle, *const Data, *const Data) -> Status;
pub type Primitive = unsafe extern "system" fn(Handle, Handle, Handle, *const Data, *mut Data, u32, u8) -> Status;

#[repr(C, packed)]
pub struct AppInterface {
    pub header: Header,
    pub create: Option<Create>, pub destroy: Option<AppAction>,
    pub suspend: Option<AppAction>, pub resume: Option<AppAction>,
    pub get_version: Option<GetText>, pub command: Option<Command>,
    pub get_intro: Option<Intro>, pub get_description: Option<GetText>,
    pub get_name: Option<GetText>, pub get_status: Option<GetStatus>,
    pub participant_create: Option<ParticipantCreate>, pub participant_destroy: Option<ParticipantDestroy>,
    pub domain_create: Option<DomainCreate>, pub domain_destroy: Option<DomainDestroy>,
    pub event: Option<Event>,
    // Unused ESIF function slots are opaque: they must never be invoked as a guessed signature.
    pub get_config: *const c_void, pub set_config: *const c_void,
    pub primitive: Option<Primitive>, pub write_log: *const c_void,
    pub register_event: *const c_void, pub unregister_event: *const c_void,
    pub send_event: *const c_void, pub send_command: *const c_void,
}

#[repr(C, packed)]
pub struct ParticipantData {
    pub version: u8, pub reserved: [u8; 3],
    pub driver_type: Data, pub device_type: Data, pub name: Data, pub description: Data,
    pub driver_name: Data, pub device_name: Data, pub device_path: Data,
    pub domain_count: u8, pub reserved2: [u8; 3], pub bus_enumerator: u32,
    pub acpi_device: Data, pub acpi_scope: Data, pub acpi_uid: Data, pub acpi_type: u32,
    pub pci_vendor: u16, pub pci_device: u16,
    pub pci_bus: u8, pub pci_device_number: u8, pub pci_function: u8, pub pci_revision: u8,
    pub pci_class: u8, pub pci_subclass: u8, pub pci_programming_interface: u8, pub reserved3: u8,
}
#[repr(C, packed)]
pub struct DomainData {
    pub version: u8, pub reserved: [u8; 3],
    pub name: Data, pub description: Data, pub guid: Data,
    pub kind: u32, pub capability: u32, pub capability_bytes: [u8; 32],
}
#[repr(C, packed)]
pub struct SessionInfo {
    pub revision: u32, pub name: [u8; 64], pub version: [u8; 20],
    pub description: [u8; 64], pub intro: [u8; 1024],
    pub server_address: [u8; 260], pub sdk_version: [u8; 20],
}
pub type Init = unsafe extern "system" fn() -> Status;
pub type Exit = unsafe extern "system" fn();
pub type SetAppInterface = unsafe extern "system" fn(*const AppInterface) -> Status;
pub type SessionCreate = unsafe extern "system" fn(*const SessionInfo) -> Handle;
pub type SessionAction = unsafe extern "system" fn(Handle);
pub type SessionConnect = unsafe extern "system" fn(Handle) -> Status;
#[repr(C, packed)]
pub struct IpcInterface {
    pub header: Header, pub version: [u8; 20], pub sdk_version: [u8; 20], pub client_sdk_version: [u8; 20],
    pub init: Option<Init>, pub exit: Option<Exit>, pub set_app_interface: Option<SetAppInterface>,
    pub create: Option<SessionCreate>, pub destroy: Option<SessionAction>,
    pub connect: Option<SessionConnect>, pub disconnect: Option<SessionAction>, pub wait_for_stop: Option<SessionAction>,
}
pub type GetInterface = unsafe extern "system" fn(*mut IpcInterface) -> Status;

/// Fixed-size, NUL-terminated SDK fields. Reject truncation and embedded NULs.
pub fn text_field<const N: usize>(text: &str) -> Result<[u8; N], &'static str> {
    if text.len() >= N || text.as_bytes().contains(&0) { return Err("IPF text field exceeds capacity or contains NUL"); }
    let mut out = [0; N]; out[..text.len()].copy_from_slice(text.as_bytes()); Ok(out)
}

impl IpcInterface {
    pub fn request(client_sdk: &str) -> Result<Self, &'static str> {
        if client_sdk.is_empty() { return Err("A verified client SDK version is required"); }
        Ok(Self {
            header: Header { kind: 8, version: 1, size: 132 },
            version: [0; 20], sdk_version: [0; 20], client_sdk_version: text_field(client_sdk)?,
            init: None, exit: None, set_app_interface: None, create: None, destroy: None,
            connect: None, disconnect: None, wait_for_stop: None,
        })
    }

    /// Validate the returned table before any function pointer is invoked.
    pub fn validate(&self) -> Result<(), &'static str> {
        let h = self.header;
        if h.kind != 8 || h.version != 1 || h.size != 132 {
            return Err("Unexpected IPF interface header");
        }
        let (init,exit,set,create,destroy,connect,disconnect,wait) =
            (self.init,self.exit,self.set_app_interface,self.create,self.destroy,self.connect,self.disconnect,self.wait_for_stop);
        if init.is_none() || exit.is_none() || set.is_none() || create.is_none()
            || destroy.is_none() || connect.is_none() || disconnect.is_none() || wait.is_none() {
            return Err("Incomplete IPF function table");
        }
        Ok(())
    }
}

impl SessionInfo {
    pub fn new(client_sdk: &str, elevated: bool) -> Result<Self, &'static str> {
        if client_sdk.is_empty() { return Err("A verified client SDK version is required"); }
        Ok(Self {
            revision: 1, name: text_field("AcerPowerProbe")?, version: text_field(env!("CARGO_PKG_VERSION"))?,
            description: text_field("Supervised CPU power validation")?,
            intro: text_field("Independent IPF power probe")?,
            server_address: text_field(if elevated { "pipe://ipfsrv.elevated" } else { "pipe://ipfsrv.public" })?,
            sdk_version: text_field(client_sdk)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::{size_of, offset_of};
    #[test]
    fn sdk_x64_layout_matches_fixed_headers() {
        assert_eq!(size_of::<Header>(), 8); assert_eq!(size_of::<Data>(), 20);
        assert_eq!(offset_of!(Data, buffer), 4); assert_eq!(offset_of!(Data, capacity), 12);
        assert_eq!(size_of::<AppData>(), 24);
        assert_eq!(size_of::<AppInterface>(), 192);
        assert_eq!(offset_of!(AppInterface, create), 8);
        assert_eq!(offset_of!(AppInterface, participant_create), 88);
        assert_eq!(offset_of!(AppInterface, domain_create), 104);
        assert_eq!(offset_of!(AppInterface, event), 120);
        assert_eq!(offset_of!(AppInterface, primitive), 144);
        assert_eq!(offset_of!(AppInterface, send_command), 184);
        assert_eq!(size_of::<ParticipantData>(), 228);
        assert_eq!(offset_of!(ParticipantData, name), 44);
        assert_eq!(offset_of!(ParticipantData, domain_count), 144);
        assert_eq!(offset_of!(ParticipantData, acpi_device), 152);
        assert_eq!(offset_of!(ParticipantData, pci_vendor), 216);
        assert_eq!(size_of::<DomainData>(), 104);
        assert_eq!(offset_of!(DomainData, kind), 64);
        assert_eq!(size_of::<SessionInfo>(), 1456);
        assert_eq!(offset_of!(SessionInfo, server_address), 1176);
        assert_eq!(offset_of!(SessionInfo, sdk_version), 1436);
        assert_eq!(size_of::<IpcInterface>(), 132);
        assert_eq!(offset_of!(IpcInterface, init), 68);
        assert_eq!(offset_of!(IpcInterface, wait_for_stop), 124);
    }
    #[test]
    fn request_has_no_callable_function_before_negotiation() {
        assert!(IpcInterface::request("").is_err());
        let r = IpcInterface::request("test-sdk").unwrap();
        let h = r.header; let kind = h.kind; let version = h.version; let size = h.size;
        assert_eq!((kind,version,size), (8,1,132));
        let (init,create,connect) = (r.init,r.create,r.connect);
        assert!(init.is_none() && create.is_none() && connect.is_none());
        assert!(r.validate().is_err());
        assert_eq!(&r.client_sdk_version[..9], b"test-sdk\0");
        assert!(text_field::<4>("abcd").is_err());
        assert!(text_field::<4>("a\0").is_err());
        assert_eq!(text_field::<4>("abc").unwrap(), *b"abc\0");
    }
    #[test]
    fn session_selects_explicit_endpoint_without_opening_it() {
        assert!(SessionInfo::new("",false).is_err());
        let read = SessionInfo::new("test-sdk",false).unwrap();
        let write = SessionInfo::new("test-sdk",true).unwrap();
        let revision = read.revision;
        assert_eq!(revision,1);
        assert_eq!(read.server_address,text_field::<260>("pipe://ipfsrv.public").unwrap());
        assert_eq!(write.server_address,text_field::<260>("pipe://ipfsrv.elevated").unwrap());
        assert_eq!(read.sdk_version,text_field::<20>("test-sdk").unwrap());
    }
}
