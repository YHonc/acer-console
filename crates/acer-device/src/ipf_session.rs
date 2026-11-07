//! Explicit native IPF session. Constructing the session loads a DLL and connects;
//! tests use only request/result marshalling and never construct a live session.
use crate::{cpu_power::Error, ipf_abi::*, ipf_callbacks::{self, Lease}};
pub use crate::ipf_callbacks::{Domain, Participant};
use acer_protocol::ipf_power::{Access, Limit, Operation, Value};
use std::{os::windows::ffi::OsStrExt, path::Path, time::{Duration, Instant}};
use windows::{core::{PCSTR, PCWSTR}, Win32::{Foundation::{FreeLibrary, HMODULE}, System::LibraryLoader::{GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, LOAD_LIBRARY_SEARCH_SYSTEM32}}};

/// Client protocol revision from the fixed Intel source used to implement this ABI.
/// Do not substitute the server's build or SDK string to bypass negotiation.
pub const CLIENT_SDK_VERSION: &str = "1.0.11402";

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MachineIdentity { pub uuid:String, pub model:String, pub bios:String }
/// Read motherboard identity for binding a recovery receipt; never called by help.
pub fn machine_identity()->Result<MachineIdentity,Error> {
    #[derive(serde::Deserialize)] struct Product { #[serde(rename="UUID")] uuid:String, #[serde(rename="Name")] model:String }
    #[derive(serde::Deserialize)] struct Bios { #[serde(rename="SMBIOSBIOSVersion")] version:String }
    let connection=wmi::WMIConnection::with_namespace_path("ROOT\\CIMV2").map_err(|e|local("identity",e.to_string()))?;
    let mut products:Vec<Product>=connection.raw_query("SELECT UUID, Name FROM Win32_ComputerSystemProduct").map_err(|e|local("identity",e.to_string()))?;
    let mut bios:Vec<Bios>=connection.raw_query("SELECT SMBIOSBIOSVersion FROM Win32_BIOS").map_err(|e|local("identity",e.to_string()))?;
    if products.len()!=1 || bios.len()!=1 {return Err(local("identity","Expected exactly one system identity and BIOS"));}
    let p=products.remove(0);let bios=bios.remove(0).version;
    let compact=p.uuid.replace('-',"");
    if compact.len()!=32 || !compact.bytes().all(|c|c.is_ascii_hexdigit()) || compact.bytes().all(|c|c==b'0') || compact.bytes().all(|c|c==b'f'||c==b'F') || p.model.trim().is_empty() || bios.trim().is_empty() {
        return Err(local("identity","System identity is missing or not unique"));
    }
    Ok(MachineIdentity {uuid:p.uuid.to_uppercase(),model:p.model,bios})
}

fn local(op: &str, message: impl Into<String>) -> Error { Error { operation: op.into(), code: None, message: message.into() } }
fn status(op: &str, code: Status) -> Result<(), Error> {
    if code == 0 { Ok(()) } else { Err(Error { operation: op.into(),code: Some(code as u32),message: match code {
        2404=>"ESIF_E_PRIMITIVE_NOT_FOUND_IN_DSP",
        3703=>"ESIF_E_SESSION_PERMISSION_DENIED",
        _=>"IPF returned an error; original code preserved",
    }.into() }) }
}
fn optional_time(result:Result<Value,Error>)->Result<Option<i64>,Error> {
    match result {
        Ok(Value::TimeWindowMs(v))=>Ok(Some(v)),
        Err(e) if e.code==Some(2404)=>Ok(None),
        Err(e)=>Err(e),
        _=>Err(local("time","Unexpected primitive value type")),
    }
}
struct Library(HMODULE);
impl Drop for Library { fn drop(&mut self) { unsafe { let _ = FreeLibrary(self.0); } } }
pub struct Session {
    // Fields remain alive through Drop's disconnect/destroy/exit sequence.
    _library: Library, lease: Lease, _app: Box<AppInterface>, _info: Box<SessionInfo>,
    ipc: IpcInterface, handle: Option<Handle>, initialized: bool, access: Access,
}
impl Session {
    /// Caller supplies an installed absolute DLL path. The client SDK is fixed to
    /// the implementation source revision and checked by GetIpcInterface.
    /// This method is not invoked by UI/startup or by offline tests.
    pub fn connect(dll: &Path, access: Access) -> Result<Self, Error> {
        let client_sdk = CLIENT_SDK_VERSION;
        if !dll.is_absolute() || dll.file_name().and_then(|v| v.to_str()) != Some("ipfipc.dll") {
            return Err(local("load", "An absolute installed ipfipc.dll path is required"));
        }
        let mut ipc = IpcInterface::request(client_sdk).map_err(|e| local("sdk",e))?;
        let info = Box::new(SessionInfo::new(client_sdk,access == Access::SupervisedWrite).map_err(|e| local("session",e))?);
        let lease = Lease::acquire().map_err(|e| local("ownership",e))?;
        let mut wide: Vec<u16> = dll.as_os_str().encode_wide().collect();
        if wide.contains(&0) { return Err(local("load", "DLL path contains NUL")); }
        wide.push(0);
        let library = Library(unsafe { LoadLibraryExW(PCWSTR(wide.as_ptr()),None,LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32) }
            .map_err(|e| local("load",e.to_string()))?);
        let export = unsafe { GetProcAddress(library.0, PCSTR(b"GetIpcInterface\0".as_ptr())) }
            .ok_or_else(|| local("resolve","GetIpcInterface missing"))?;
        let get: GetInterface = unsafe { std::mem::transmute(export) };
        status("GetIpcInterface",unsafe { get(&mut ipc) })?;
        ipc.validate().map_err(|e| local("interface",e))?;
        let mut session = Self { _library: library,lease,_app: Box::new(ipf_callbacks::interface()),_info: info,ipc,handle: None,initialized: false,access };
        let init = session.ipc.init.unwrap();
        status("Ipc_Init",unsafe { init() })?;
        session.initialized = true;
        let set = session.ipc.set_app_interface.unwrap();
        status("Ipc_SetAppIface",unsafe { set(&*session._app) })?;
        let create = session.ipc.create.unwrap();
        let handle = unsafe { create(&*session._info) };
        if handle == u64::MAX { return Err(local("IpcSession_Create","Invalid session handle")); }
        session.handle = Some(handle);
        let connect = session.ipc.connect.unwrap();
        status("IpcSession_Connect",unsafe { connect(handle) })?;
        Ok(session)
    }
    pub fn participants(&self) -> Result<Vec<Participant>, Error> { self.lease.participants().map_err(|e| local("discovery",e)) }
    /// PPCC is not allowed by the audited PUBLIC/ELEVATED ACL. Preserve the
    /// server's denial; never replace it with guessed ranges or cached maxima.
    pub fn capabilities(&mut self, participant: Handle, domain: Handle) -> Result<Vec<acer_protocol::ipf_ppcc::Capability>,Error> {
        let (esif,primitive)=self.lease.primitive(participant,domain).map_err(|e|local("discovery",e))?;
        let mut buffer=[0u8;12+4*72];
        let request=Data {kind:24,buffer:std::ptr::null_mut(),capacity:0,length:0};
        let mut response=Data {kind:7,buffer:buffer.as_mut_ptr().cast(),capacity:buffer.len() as u32,length:0};
        status("GET_RAPL_POWER_CONTROL_CAPABILITIES",unsafe { primitive(esif,participant,domain,&request,&mut response,75,255) })?;
        if response.kind!=7 || response.buffer!=buffer.as_mut_ptr().cast() || response.capacity!=buffer.len() as u32 || response.length>buffer.len() as u32 {
            return Err(local("PPCC","Unexpected capability response descriptor"));
        }
        acer_protocol::ipf_ppcc::decode(&buffer[..response.length as usize]).map_err(|e|local("PPCC",format!("{e:?}")))
    }
    /// Wait only for a caller-selected observed participant/domain, never infer CPU
    /// from position zero or silently substitute a different domain.
    pub fn wait_for_domain(&self, participant: Handle, domain: Handle, timeout: Duration) -> Result<(), Error> {
        let start = Instant::now();
        loop {
            if self.lease.primitive(participant,domain).is_ok() { return Ok(()); }
            if start.elapsed() >= timeout { return Err(local("discovery","Selected domain was not discovered before timeout")); }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    /// Restricted primitive invocation; write calls remain crate-private until
    /// capability checks and durable recovery receipts are connected above it.
    pub(crate) fn execute(&mut self, participant: Handle, domain: Handle, limit: Limit, operation: Operation) -> Result<Value, Error> {
        let (esif,primitive) = self.lease.primitive(participant,domain).map_err(|e| local("discovery",e))?;
        let mut call = Call::new(operation,self.access)?;
        let mut request = call.request(); let mut response = call.response();
        let instance = match limit { Limit::Pl1 => 0, Limit::Pl2 => 1 };
        status("Primitive",unsafe { primitive(esif,participant,domain,&mut request,&mut response,operation.primitive(),instance) })?;
        call.decode(response)
    }
    pub fn read(&mut self, participant: Handle, domain: Handle, limit: Limit, operation: Operation) -> Result<Value, Error> {
        if operation.is_write() { return Err(local("read","Setter is not accepted by the read entry point")); }
        self.execute(participant,domain,limit,operation)
    }
    /// Full transaction orchestration. A failed/denied PPCC read aborts before
    /// persistence and before setters. Persistence is supplied by the native CLI.
    pub fn trial(&mut self, participant: Handle, domain: Handle, changes: &[crate::cpu_power::Change],
        persist: impl FnMut(&[crate::cpu_power::Checkpoint])->Result<(),Error>) -> Result<crate::cpu_power::Report,Error> {
        if self.access!=Access::SupervisedWrite { return Err(local("policy","A supervised write session is required")); }
        let mut io=CpuBackend {session:self,participant,domain};
        crate::cpu_power::trial(&mut io,changes,persist)
    }
    pub fn recover(&mut self,participant:Handle,domain:Handle,points:&[crate::cpu_power::Checkpoint])->Result<crate::cpu_power::RecoveryReport,Error> {
        if self.access!=Access::SupervisedWrite {return Err(local("policy","A supervised write session is required"));}
        crate::cpu_power::recover(&mut CpuBackend {session:self,participant,domain},points)
    }
}
struct CpuBackend<'a> { session: &'a mut Session, participant:Handle,domain:Handle }
impl crate::cpu_power::CpuIo for CpuBackend<'_> {
    fn read(&mut self,limit:Limit)->Result<crate::cpu_power::State,Error> {
        let power=self.session.execute(self.participant,self.domain,limit,Operation::GetLimit)?;
        let enabled=self.session.execute(self.participant,self.domain,limit,Operation::GetEnabled)?;
        let time_ms=optional_time(self.session.execute(self.participant,self.domain,limit,Operation::GetTimeWindow))?;
        let (Value::LimitMw(power_mw),Value::Enabled(enabled))=(power,enabled) else {
            return Err(local("read","Unexpected primitive value type"));
        };
        Ok(crate::cpu_power::State {power_mw,enabled,time_ms})
    }
    fn capability(&mut self,limit:Limit)->Result<crate::cpu_power::Capability,Error> {
        let index=match limit {Limit::Pl1=>0,Limit::Pl2=>1};
        let rows=self.session.capabilities(self.participant,self.domain)?;
        let row=rows.iter().find(|c|c.index==index).ok_or_else(||local("PPCC","Requested PL has no explicit capability row"))?;
        Ok(crate::cpu_power::Capability {min_mw:row.min_mw,max_mw:row.max_mw,time_bounds_ms:row.time_bounds_ms.map(|(a,b)|(i64::from(a),i64::from(b)))})
    }
    fn write(&mut self,limit:Limit,operation:Operation)->Result<(),Error> {
        if !operation.is_write() { return Err(local("write","Only setter operations are accepted")); }
        match self.session.execute(self.participant,self.domain,limit,operation)? {
            Value::Acknowledged=>Ok(()),_=>Err(local("write","Unexpected setter response")),
        }
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            let disconnect = self.ipc.disconnect.unwrap(); let destroy = self.ipc.destroy.unwrap();
            unsafe { disconnect(handle); destroy(handle); }
        }
        if self.initialized { let exit = self.ipc.exit.unwrap(); unsafe { exit(); } }
    }
}

struct Call { operation: Operation, input: [u8; 8], output: [u8; 8], input_kind: u32, input_len: u32, output_kind: u32, output_len: u32 }
impl Call {
    fn new(operation: Operation, access: Access) -> Result<Self, Error> {
        if operation.is_write() && access != Access::SupervisedWrite { return Err(local("policy","Read-only IPF session")); }
        let mut call = Self { operation,input: [0;8],output: [0;8],input_kind:24,input_len:0,output_kind:24,output_len:0 };
        match operation {
            Operation::GetLimit => { call.output_kind=26; call.output_len=4; }
            Operation::GetEnabled => { call.output_kind=3; call.output_len=4; }
            Operation::GetTimeWindow => { call.output_kind=31; call.output_len=8; }
            Operation::SetLimitMw(0) => return Err(local("value","Zero power limit is invalid")),
            Operation::SetLimitMw(v) => { call.input_kind=26;call.input_len=4;call.input[..4].copy_from_slice(&v.to_le_bytes()); }
            Operation::SetEnabled(v) => { call.input_kind=3;call.input_len=4;call.input[..4].copy_from_slice(&u32::from(v).to_le_bytes()); }
            Operation::SetTimeWindowMs(v) => {
                if v < 0 { return Err(local("value","Negative time window")); }
                call.input_kind=31;call.input_len=8;call.input=v.to_le_bytes();
            }
        } Ok(call)
    }
    fn request(&mut self) -> Data { Data { kind:self.input_kind,buffer: if self.input_len==0 { std::ptr::null_mut() } else { self.input.as_mut_ptr().cast() },capacity:self.input_len,length:self.input_len } }
    fn response(&mut self) -> Data { Data { kind:self.output_kind,buffer: if self.output_len==0 { std::ptr::null_mut() } else { self.output.as_mut_ptr().cast() },capacity:self.output_len,length:0 } }
    fn decode(&self, response: Data) -> Result<Value, Error> {
        let expected_ptr = if self.output_len==0 { std::ptr::null_mut() } else { self.output.as_ptr().cast_mut().cast() };
        if response.kind!=self.output_kind || response.buffer!=expected_ptr || response.capacity!=self.output_len || response.length>self.output_len {
            return Err(local("decode","IPF response descriptor changed type, pointer or capacity"));
        }
        let word = u32::from_le_bytes(self.output[..4].try_into().unwrap());
        match (self.operation,response.length) {
            (Operation::GetLimit,4) => Ok(Value::LimitMw(word)),
            (Operation::GetEnabled,4) => Ok(Value::Enabled(word>0)),
            (Operation::GetTimeWindow,4) => Ok(Value::TimeWindowMs(i64::from(word))),
            (Operation::GetTimeWindow,8) => { let v=i64::from_le_bytes(self.output); if v<0 { Err(local("decode","Negative time response")) } else { Ok(Value::TimeWindowMs(v)) } }
            (op,0) if op.is_write() => Ok(Value::Acknowledged),
            _ => Err(local("decode","Invalid IPF response length")),
        }
    }
}
#[cfg(test)] mod tests {
    use super::*;
    #[test] fn only_missing_primitive_can_be_an_absent_time_window() {
        assert_eq!(optional_time(Ok(Value::TimeWindowMs(56000))).unwrap(),Some(56000));
        assert_eq!(optional_time(Err(status("time",2404).unwrap_err())).unwrap(),None);
        for code in [3703,2600,2402] {assert!(optional_time(Err(status("time",code).unwrap_err())).is_err());}
        assert!(optional_time(Ok(Value::Enabled(true))).is_err());
    }
    #[test] fn primitive_marshalling_is_typed_and_write_gated() {
        assert!(Call::new(Operation::SetLimitMw(50000),Access::ReadOnly).is_err());
        let mut call=Call::new(Operation::SetLimitMw(50000),Access::SupervisedWrite).unwrap();
        let request=call.request(); let (kind,len)=(request.kind,request.length);
        assert_eq!((kind,len),(26,4)); assert_eq!(&call.input[..4],&50000u32.to_le_bytes());
        let response=call.response(); assert_eq!(call.decode(response).unwrap(),Value::Acknowledged);
        assert!(Call::new(Operation::SetTimeWindowMs(-1),Access::SupervisedWrite).is_err());
    }
    #[test] fn time_readback_accepts_both_sdk_widths_but_not_missing_or_negative() {
        let mut call=Call::new(Operation::GetTimeWindow,Access::ReadOnly).unwrap();
        let mut response=call.response(); assert!(call.decode(response).is_err());
        call.output=28000i64.to_le_bytes(); response.length=4;
        assert_eq!(call.decode(response).unwrap(),Value::TimeWindowMs(28000));
        response.length=8; assert_eq!(call.decode(response).unwrap(),Value::TimeWindowMs(28000));
        call.output=(-1i64).to_le_bytes(); assert!(call.decode(response).is_err());
        response.buffer=std::ptr::null_mut(); assert!(call.decode(response).is_err());
    }
}
