//! One process-owned callback registry. The transport owner must stop/join IPC
//! callbacks before releasing its lease or unloading the DLL.
#![allow(dead_code)]
use crate::ipf_abi::*;
use std::{collections::BTreeMap, marker::PhantomData, rc::Rc, sync::Mutex};

const INVALID: Status = 2300;
const SMALL: Status = 1300;
const APP_HANDLE: Handle = 2;
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Domain { pub handle: Handle, pub name: String, pub kind: u32, pub capability: u32 }
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Participant { pub handle: Handle, pub name: String, pub description: String, pub domains: BTreeMap<Handle, Domain> }
#[derive(Default)]
struct Registry { esif: Option<Handle>, primitive: Option<Primitive>, participants: BTreeMap<Handle, Participant> }
static REGISTRY: Mutex<Option<Registry>> = Mutex::new(None);

/// Thread-bound lease; one active IPF client per process.
pub struct Lease { _thread: PhantomData<Rc<()>> }
impl Lease {
    pub fn acquire() -> Result<Self, &'static str> {
        let mut slot = REGISTRY.lock().map_err(|_| "IPF callback registry poisoned")?;
        if slot.is_some() { return Err("An IPF session already owns this process"); }
        *slot = Some(Registry::default()); Ok(Self { _thread: PhantomData })
    }
    pub fn participants(&self) -> Result<Vec<Participant>, &'static str> {
        let slot = REGISTRY.lock().map_err(|_| "IPF callback registry poisoned")?;
        let state = slot.as_ref().ok_or("IPF callback registry missing")?;
        Ok(state.participants.values().cloned().collect())
    }
    pub fn primitive(&self, participant: Handle, domain: Handle) -> Result<(Handle, Primitive), &'static str> {
        let slot = REGISTRY.lock().map_err(|_| "IPF callback registry poisoned")?;
        let state = slot.as_ref().ok_or("IPF callback registry missing")?;
        if !state.participants.get(&participant).is_some_and(|p| p.domains.contains_key(&domain)) {
            return Err("IPF participant/domain was not discovered or was removed");
        }
        Ok((state.esif.ok_or("AppCreate has not completed")?, state.primitive.ok_or("Primitive callback missing")?))
    }
}
impl Drop for Lease {
    fn drop(&mut self) { if let Ok(mut slot) = REGISTRY.lock() { *slot = None; } }
}
// Runtime-issued handles are opaque; only ESIF_INVALID_HANDLE is rejected.
fn valid_handle(h: Handle) -> bool { h != u64::MAX }
fn mutate(app: Handle, f: impl FnOnce(&mut Registry) -> Status) -> Status {
    if app != APP_HANDLE { return INVALID; }
    // No Rust unwind may cross the C ABI. Poisoned state fails closed.
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let Ok(mut slot) = REGISTRY.lock() else { return INVALID; };
        let Some(state) = slot.as_mut() else { return INVALID; };
        f(state)
    })).unwrap_or(INVALID)
}
/// Copies SDK-owned strings during the callback; no borrowed pointers survive it.
unsafe fn read_text(data: Data) -> Result<String, Status> {
    if data.kind != 8 || data.length > data.capacity || data.length > 16_384 { return Err(INVALID); }
    if data.length == 0 { return Ok(String::new()); }
    if data.buffer.is_null() { return Err(INVALID); }
    let bytes = unsafe { std::slice::from_raw_parts(data.buffer.cast::<u8>(), data.length as usize) };
    let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
    std::str::from_utf8(&bytes[..end]).map(str::to_owned).map_err(|_| INVALID)
}
unsafe fn write_text(out: *mut Data, bytes: &[u8]) -> Status {
    if out.is_null() { return INVALID; }
    let mut data = unsafe { out.read_unaligned() };
    data.length = bytes.len() as u32;
    unsafe { out.write_unaligned(data); }
    if data.capacity < data.length { return SMALL; }
    if data.buffer.is_null() { return INVALID; }
    unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), data.buffer.cast(), bytes.len()); }
    0
}
unsafe extern "system" fn name(out: *mut Data) -> Status { unsafe { write_text(out, b"AcerPowerProbe\0") } }
unsafe extern "system" fn description(out: *mut Data) -> Status { unsafe { write_text(out, b"Supervised CPU power validation\0") } }
unsafe extern "system" fn version(out: *mut Data) -> Status { unsafe { write_text(out, concat!(env!("CARGO_PKG_VERSION"), "\0").as_bytes()) } }
unsafe extern "system" fn intro(_: Handle, out: *mut Data) -> Status { unsafe { description(out) } }
unsafe extern "system" fn create(iface: *mut AppInterface, esif: Handle, app: *mut Handle, _: *const AppData, _: i32) -> Status {
    if iface.is_null() || app.is_null() || !valid_handle(esif) { return INVALID; }
    let header = unsafe { std::ptr::addr_of!((*iface).header).read_unaligned() };
    if header.kind != 0 || header.version != 4 || header.size != 192 { return INVALID; }
    let primitive = unsafe { std::ptr::addr_of!((*iface).primitive).read_unaligned() };
    if primitive.is_none() { return INVALID; }
    mutate(APP_HANDLE, |state| {
        if state.esif.is_some() { return INVALID; }
        state.esif = Some(esif); state.primitive = primitive;
        unsafe { app.write_unaligned(APP_HANDLE); } 0
    })
}
unsafe extern "system" fn destroy(app: Handle) -> Status {
    mutate(app, |s| { *s = Registry::default(); 0 })
}
unsafe extern "system" fn action(app: Handle) -> Status { mutate(app, |_| 0) }
unsafe extern "system" fn participant_create(app: Handle, handle: Handle, data: *const ParticipantData, _: i32) -> Status {
    if data.is_null() || !valid_handle(handle) { return INVALID; }
    mutate(app, |s| {
        if s.esif.is_none() || s.participants.contains_key(&handle) { return INVALID; }
        let name = unsafe { read_text(std::ptr::addr_of!((*data).name).read_unaligned()) };
        let description = unsafe { read_text(std::ptr::addr_of!((*data).description).read_unaligned()) };
        let (Ok(name),Ok(description)) = (name,description) else { return INVALID; };
        s.participants.insert(handle, Participant { handle,name,description,domains: BTreeMap::new() }); 0
    })
}
unsafe extern "system" fn participant_destroy(app: Handle, handle: Handle) -> Status {
    mutate(app, |s| { s.participants.remove(&handle); 0 })
}
unsafe extern "system" fn domain_create(app: Handle, participant: Handle, handle: Handle, data: *const DomainData, _: i32) -> Status {
    if data.is_null() || !valid_handle(handle) { return INVALID; }
    mutate(app, |s| {
        let Some(p) = s.participants.get_mut(&participant) else { return INVALID; };
        if p.domains.contains_key(&handle) { return INVALID; }
        let Ok(name) = (unsafe { read_text(std::ptr::addr_of!((*data).name).read_unaligned()) }) else { return INVALID; };
        let kind = unsafe { std::ptr::addr_of!((*data).kind).read_unaligned() };
        let capability = unsafe { std::ptr::addr_of!((*data).capability).read_unaligned() };
        p.domains.insert(handle, Domain { handle,name,kind,capability }); 0
    })
}
unsafe extern "system" fn domain_destroy(app: Handle, participant: Handle, handle: Handle) -> Status {
    mutate(app, |s| { if let Some(p) = s.participants.get_mut(&participant) { p.domains.remove(&handle); } 0 })
}
pub fn interface() -> AppInterface {
    AppInterface {
        header: Header { kind: 0, version: 4, size: 192 },
        create: Some(create), destroy: Some(destroy), suspend: Some(action), resume: Some(action),
        get_version: Some(version), command: None, get_intro: Some(intro), get_description: Some(description),
        get_name: Some(name), get_status: None, participant_create: Some(participant_create),
        participant_destroy: Some(participant_destroy), domain_create: Some(domain_create), domain_destroy: Some(domain_destroy),
        event: None, get_config: std::ptr::null(), set_config: std::ptr::null(), primitive: None,
        write_log: std::ptr::null(), register_event: std::ptr::null(), unregister_event: std::ptr::null(),
        send_event: std::ptr::null(), send_command: std::ptr::null(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn text(bytes: &mut [u8]) -> Data { Data { kind: 8,buffer: bytes.as_mut_ptr().cast(),capacity: bytes.len() as u32,length: bytes.len() as u32 } }
    unsafe extern "system" fn fake_primitive(_: Handle,_: Handle,_: Handle,_: *const Data,_: *mut Data,_: u32,_: u8) -> Status { 0 }
    #[test]
    fn callbacks_discover_and_remove_runtime_handles() {
        let lease = Lease::acquire().unwrap(); assert!(Lease::acquire().is_err());
        let mut iface = interface(); iface.primitive = Some(fake_primitive); let mut app = 0;
        unsafe {
            assert_eq!(create(&mut iface,99,&mut app,std::ptr::null(),0),0);
            assert_eq!(app,APP_HANDLE);
            let mut p: ParticipantData = std::mem::zeroed();
            // Keep source buffers alive across callbacks.
            let mut pname = *b"CPU\0"; let mut desc = *b"Processor\0";
            p.name = text(&mut pname); p.description = text(&mut desc);
            assert_eq!(participant_create(app,100,&p,0),0);
            let mut d: DomainData = std::mem::zeroed(); let mut dname = *b"D0\0";
            d.name = text(&mut dname); d.kind = 0; d.capability = 4;
            assert_eq!(domain_create(app,100,101,&d,0),0);
            assert_eq!(lease.participants().unwrap()[0].name,"CPU");
            assert_eq!(lease.primitive(100,101).unwrap().0,99);
            assert_eq!(domain_destroy(app,100,101),0);
            assert!(lease.primitive(100,101).is_err());
            assert_eq!(participant_destroy(app,100),0);
            assert!(lease.participants().unwrap().is_empty());
            assert_eq!(destroy(app),0);
            assert!(lease.primitive(100,101).is_err());
        }
    }
    #[test]
    fn text_output_preserves_capacity_and_rejects_bad_input() {
        let mut buffer = [0; 2]; let mut data = text(&mut buffer);
        assert_eq!(unsafe { name(&mut data) }, SMALL);
        let (cap,len) = (data.capacity,data.length); assert_eq!(cap,2); assert_eq!(len,15);
        assert_eq!(buffer,[0,0]);
        assert!(unsafe { read_text(data) }.is_err());
        let mut buffer = [0; 32]; let mut data = text(&mut buffer);
        assert_eq!(unsafe { name(&mut data) },0);
        assert_eq!(unsafe { read_text(data) }.unwrap(),"AcerPowerProbe");
    }
}
