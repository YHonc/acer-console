//! Narrow Win32 ABI surface. Handles and LocalAlloc strings are RAII-owned.
use super::*;
use std::{ffi::c_void, mem::{size_of, zeroed}, ptr::{null, null_mut}, time::{Duration, Instant}};
type Handle = *mut c_void;
#[repr(C)] struct SecurityAttributes { length: u32, descriptor: *mut c_void, inherit: i32 }
#[repr(C)] struct SidAttributes { sid: *mut c_void, attributes: u32 }
#[repr(C)] struct ShellInfo {
    size: u32, mask: u32, hwnd: Handle, verb: *const u16, file: *const u16,
    parameters: *const u16, directory: *const u16, show: i32, instance: Handle,
    id_list: *mut c_void, class: *const u16, class_key: Handle, hot_key: u32,
    icon: Handle, process: Handle,
}
#[link(name="kernel32")] unsafe extern "system" {
    fn CloseHandle(h: Handle) -> i32;
    fn GetLastError() -> u32;
    fn LocalFree(p: *mut c_void) -> *mut c_void;
    fn GetCurrentProcess() -> Handle;
    fn OpenProcess(access: u32, inherit: i32, pid: u32) -> Handle;
    fn GetProcessId(h: Handle) -> u32;
    fn GetExitCodeProcess(h: Handle, code: *mut u32) -> i32;
    fn CreateNamedPipeW(name: *const u16, mode: u32, pipe_mode: u32, instances: u32, out_size: u32, in_size: u32, timeout: u32, security: *const SecurityAttributes) -> Handle;
    fn ConnectNamedPipe(pipe: Handle, overlapped: *mut c_void) -> i32;
    fn CreateFileW(name: *const u16, access: u32, share: u32, security: *const SecurityAttributes, creation: u32, flags: u32, template: Handle) -> Handle;
    fn SetNamedPipeHandleState(pipe: Handle, mode: *const u32, count: *const u32, timeout: *const u32) -> i32;
    fn GetNamedPipeClientProcessId(pipe: Handle, pid: *mut u32) -> i32;
    fn GetNamedPipeServerProcessId(pipe: Handle, pid: *mut u32) -> i32;
    fn ReadFile(h: Handle, data: *mut c_void, len: u32, count: *mut u32, overlapped: *mut c_void) -> i32;
    fn WriteFile(h: Handle, data: *const c_void, len: u32, count: *mut u32, overlapped: *mut c_void) -> i32;
}
#[link(name="advapi32")] unsafe extern "system" {
    fn OpenProcessToken(h: Handle, access: u32, token: *mut Handle) -> i32;
    fn GetTokenInformation(token: Handle, class: i32, data: *mut c_void, len: u32, needed: *mut u32) -> i32;
    fn ConvertSidToStringSidW(sid: *const c_void, text: *mut *mut u16) -> i32;
    fn ConvertStringSecurityDescriptorToSecurityDescriptorW(text: *const u16, revision: u32, descriptor: *mut *mut c_void, size: *mut u32) -> i32;
}
#[link(name="shell32")] unsafe extern "system" { fn ShellExecuteExW(info: *mut ShellInfo) -> i32; }
#[link(name="bcrypt")] unsafe extern "system" { fn BCryptGenRandom(algorithm: Handle, buffer: *mut u8, length: u32, flags: u32) -> i32; }
#[link(name="ole32")] unsafe extern "system" {
    fn CoInitializeEx(reserved: *mut c_void, flags: u32) -> i32;
    fn CoUninitialize();
}

struct Owned(isize);
impl Owned {
    fn new(h: Handle) -> Result<Self, DeviceError> { if h.is_null() || h as isize == -1 { Err(last_error()) } else { Ok(Self(h as isize)) } }
    fn raw(&self) -> Handle { self.0 as Handle }
}
impl Drop for Owned { fn drop(&mut self) { unsafe { CloseHandle(self.raw()); } } }
struct Local(*mut c_void);
impl Drop for Local { fn drop(&mut self) { unsafe { LocalFree(self.0); } } }
fn wide(s: &str) -> Vec<u16> { s.encode_utf16().chain(Some(0)).collect() }
fn last_error() -> DeviceError { error(std::io::Error::from_raw_os_error(unsafe { GetLastError() } as i32)) }
fn check(ok: i32) -> Result<(), DeviceError> { if ok == 0 { Err(last_error()) } else { Ok(()) } }
fn pause(deadline: Instant) -> Result<(), DeviceError> {
    if Instant::now() >= deadline { return Err(error("Broker timeout; operation outcome may be unknown")); }
    std::thread::sleep(Duration::from_millis(15)); Ok(())
}
fn token_data(token: &Owned, class: i32) -> Result<Vec<u64>, DeviceError> {
    let mut needed = 0;
    unsafe { GetTokenInformation(token.raw(), class, null_mut(), 0, &mut needed); }
    if needed == 0 || needed > 65536 { return Err(error("Invalid token information size")); }
    let mut buffer = vec![0u64; (needed as usize + 7) / 8];
    unsafe { check(GetTokenInformation(token.raw(), class, buffer.as_mut_ptr().cast(), needed, &mut needed))?; }
    Ok(buffer)
}
#[derive(PartialEq)] struct Identity { sid: String, session: u32, elevated: bool }
fn identity(process: Handle) -> Result<Identity, DeviceError> {
    let mut raw = null_mut(); unsafe { check(OpenProcessToken(process, 8, &mut raw))?; }
    let token = Owned::new(raw)?;
    let user = token_data(&token, 1)?;
    let sid = unsafe { (*(user.as_ptr().cast::<SidAttributes>())).sid };
    let mut text = null_mut(); unsafe { check(ConvertSidToStringSidW(sid, &mut text))?; }
    let _text = Local(text.cast());
    let mut length = 0; unsafe { while *text.add(length) != 0 { length += 1; } }
    let sid = unsafe { String::from_utf16_lossy(std::slice::from_raw_parts(text, length)) };
    let session = token_data(&token, 12)?[0] as u32;
    let elevated = token_data(&token, 20)?[0] as u32 != 0;
    Ok(Identity { sid, session, elevated })
}
fn process(pid: u32) -> Result<Owned, DeviceError> { unsafe { Owned::new(OpenProcess(0x1000, 0, pid)) } }
fn same_user(a: &Identity, b: &Identity) -> Result<(), DeviceError> {
    if a.sid != b.sid || a.session != b.session { return Err(error("Broker user/session mismatch (alternate-credential elevation is not supported)")); } Ok(())
}
pub(super) struct Session { pipe: Vec<u16>, helper: Owned, lock: std::sync::Mutex<()> }
pub(super) fn is_alive(session: &Session) -> Result<bool, DeviceError> {
    let mut exit_code = 0;
    unsafe { check(GetExitCodeProcess(session.helper.raw(), &mut exit_code))?; }
    // acer-helper exits only with 0 or 1; 259 is Win32 STILL_ACTIVE.
    Ok(exit_code == 259)
}
pub(super) fn launch() -> Result<Session, DeviceError> {
    std::thread::spawn(launch_sta).join().map_err(|_|error("Elevation thread stopped"))?
}
fn launch_sta() -> Result<Session, DeviceError> {
    let result = unsafe { CoInitializeEx(null_mut(), 2 | 4) };
    if result < 0 { return Err(error(format!("Shell COM initialization failed: {result:#x}"))); }
    struct Apartment;
    impl Drop for Apartment { fn drop(&mut self) { unsafe { CoUninitialize(); } } }
    let _apartment = Apartment;
    let helper = std::env::current_exe().map_err(error)?.parent().ok_or_else(||error("No executable directory"))?.join("acer-helper.exe");
    if !helper.is_file() { return Err(error("acer-helper.exe is missing beside the desktop executable")); }
    let mut bytes = [0u8; 16]; unsafe { if BCryptGenRandom(null_mut(), bytes.as_mut_ptr(), 16, 2) < 0 { return Err(error("System RNG failed")); } }
    let nonce: String = bytes.iter().map(|b|format!("{b:02x}")).collect();
    let pid = std::process::id();
    let pipe = wide(&format!(r"\\.\pipe\acer-broker-{pid}-{nonce}"));
    let verb = wide("runas"); let file = wide(helper.to_str().ok_or_else(||error("Invalid helper path"))?);
    let args = wide(&format!("--client-pid {pid} --nonce {nonce}"));
    let mut info: ShellInfo = unsafe { zeroed() };
    info.size = size_of::<ShellInfo>() as u32; info.mask = 0x40 | 0x100; // NOCLOSEPROCESS, NOASYNC
    info.verb = verb.as_ptr(); info.file = file.as_ptr(); info.parameters = args.as_ptr(); info.show = 0;
    unsafe { check(ShellExecuteExW(&mut info))?; }
    let helper = Owned::new(info.process)?;
    let me = unsafe { identity(GetCurrentProcess())? }; let peer = identity(helper.raw())?;
    same_user(&me, &peer)?;
    if !peer.elevated { return Err(error("Helper is not elevated")); }
    Ok(Session { pipe, helper, lock: std::sync::Mutex::new(()) })
}
fn transfer(h: &Owned, bytes: &mut [u8], writing: bool, deadline: Instant) -> Result<(), DeviceError> {
    let mut offset = 0;
    while offset < bytes.len() {
        if Instant::now() >= deadline { return Err(error("Pipe transfer timeout; outcome unknown")); }
        let mut count = 0;
        let ok = unsafe { if writing { WriteFile(h.raw(), bytes[offset..].as_ptr().cast(), (bytes.len()-offset) as u32, &mut count, null_mut()) } else { ReadFile(h.raw(), bytes[offset..].as_mut_ptr().cast(), (bytes.len()-offset) as u32, &mut count, null_mut()) } };
        if ok == 0 { let code = unsafe { GetLastError() }; if code != 232 && code != 536 { return Err(last_error()); } }
        offset += count as usize;
        if count == 0 { pause(deadline)?; }
    }
    Ok(())
}
fn send(h: &Owned, data: &[u8], deadline: Instant) -> Result<(), DeviceError> {
    if data.is_empty() || data.len() > MAX_FRAME { return Err(error("RPC frame exceeds limit")); }
    let mut frame = (data.len() as u32).to_le_bytes().to_vec(); frame.extend_from_slice(data);
    transfer(h, &mut frame, true, deadline)
}
fn receive(h: &Owned, deadline: Instant) -> Result<Vec<u8>, DeviceError> {
    let mut header = [0u8; 4]; transfer(h, &mut header, false, deadline)?;
    let size = u32::from_le_bytes(header) as usize;
    if size == 0 || size > MAX_FRAME { return Err(error("Invalid RPC frame length")); }
    let mut bytes = vec![0; size]; transfer(h, &mut bytes, false, deadline)?; Ok(bytes)
}
pub(super) fn exchange(session: &Session, request: &[u8], response_budget:Duration) -> Result<Vec<u8>, DeviceError> {
    let _guard = session.lock.lock().map_err(error)?;
    let deadline = Instant::now() + Duration::from_secs(10);
    let pipe = loop {
        let mut exit_code = 0;
        unsafe { check(GetExitCodeProcess(session.helper.raw(), &mut exit_code))?; }
        if exit_code != 259 { return Err(error(format!("Authorized helper exited ({exit_code}); reconnect explicitly"))); }
        let h = unsafe { CreateFileW(session.pipe.as_ptr(), 0x0012019b, 0, null(), 3, 0x00100000, null_mut()) }; // Explicit data/attribute rights, no FILE_CREATE_PIPE_INSTANCE; anonymous SQOS
        if h as isize != -1 { break Owned::new(h)?; }
        match unsafe { GetLastError() } { 2 | 231 => pause(deadline)?, _ => return Err(last_error()) }
    };
    let mut peer_pid = 0;
    unsafe { check(GetNamedPipeServerProcessId(pipe.raw(), &mut peer_pid))?; }
    if peer_pid != unsafe { GetProcessId(session.helper.raw()) } { return Err(error("Pipe server PID mismatch")); }
    let server = process(peer_pid)?; let peer = identity(server.raw())?;
    same_user(&unsafe { identity(GetCurrentProcess())? }, &peer)?;
    if !peer.elevated { return Err(error("Pipe server is not elevated")); }
    unsafe { check(SetNamedPipeHandleState(pipe.raw(), &1, null(), null()))?; }
    send(&pipe, request, deadline)?;
    // Budget is selected from the closed native operation, never caller JSON.
    let response = receive(&pipe, Instant::now() + response_budget)?;
    // The server holds the pipe until acknowledgement, avoiding blocking FlushFileBuffers.
    transfer(&pipe, &mut [0xa5], true, Instant::now() + Duration::from_secs(2))?;
    Ok(response)
}
pub(super) fn serve() -> Result<(), DeviceError> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len()!=4 || args[0]!="--client-pid" || args[2]!="--nonce" || args[3].len()!=32 || !args[3].bytes().all(|b| b.is_ascii_hexdigit()) { return Err(error("Invalid helper arguments")); }
    let client_pid: u32 = args[1].parse().map_err(error)?;
    let client = process(client_pid)?; // Keep process object alive; PID cannot be recycled.
    let expected = identity(client.raw())?;
    let me = unsafe { identity(GetCurrentProcess())? }; same_user(&me, &expected)?;
    if !me.elevated { return Err(error("Start helper through explicit runas authorization")); }
    let name = wide(&format!(r"\\.\pipe\acer-broker-{client_pid}-{}", args[3]));
    let sddl = wide(&format!("D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;0x12019b;;;{})", expected.sid));
    let mut descriptor = null_mut();
    unsafe { check(ConvertStringSecurityDescriptorToSecurityDescriptorW(sddl.as_ptr(), 1, &mut descriptor, null_mut()))?; }
    let _descriptor = Local(descriptor);
    let security = SecurityAttributes { length: size_of::<SecurityAttributes>() as u32, descriptor, inherit: 0 };
    // Device startup is also bounded. No write policy can enter through arguments/RPC.
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || { let _ = tx.send(acer_device::Device::connect_with_policy(acer_device::WritePolicy::Verified)); });
    let device = rx.recv_timeout(Duration::from_secs(15)).map_err(error)??;
    // All normal/error returns below attempt curve shutdown, including parent
    // exit, idle expiry and RPC timeout. This cannot recover from OS force-kill.
    struct ThermalCleanup(acer_device::Device);
    impl Drop for ThermalCleanup {
        fn drop(&mut self) {
            let device=self.0.clone();
            let (tx,rx)=std::sync::mpsc::sync_channel(1);
            let worker=std::thread::Builder::new().name("thermal-cleanup".into()).spawn(move||{let _=tx.send(device.stop_thermal());});
            if worker.is_err(){eprintln!("Thermal cleanup could not start; restoration unknown");return;}
            match rx.recv_timeout(Duration::from_secs(15)) {
                Ok(Ok(status)) if !status.restore_pending=>{},
                Ok(Ok(status))=>eprintln!("Thermal cleanup: {}",status.message),
                Ok(Err(e))=>eprintln!("Thermal cleanup failed: {}",e.message),
                Err(_)=>eprintln!("Thermal cleanup timed out; restoration unknown"),
            }
        }
    }
    let _thermal_cleanup=ThermalCleanup(device.clone());
    let usb=std::sync::Arc::new(super::usb_controls::UsbControls::default());
    let direct=std::sync::Arc::new(super::direct_controls::DirectControls::default());
    let audio=std::sync::Arc::new(super::audio_bridge::AudioControls::default());
    let logo=std::sync::Arc::new(super::boot_logo::BootLogoManager::default());
    let mut last_id = 0;
    loop {
        logo.expire()?;
        let pipe = unsafe { Owned::new(CreateNamedPipeW(name.as_ptr(), 3 | 0x00080000, 1 | 8, 1, MAX_FRAME as u32 + 4, MAX_FRAME as u32 + 4, 1000, &security))? };
        let idle = Instant::now() + Duration::from_secs(60);
        loop {
            let mut client_exit_code=0;
            unsafe { check(GetExitCodeProcess(client.raw(),&mut client_exit_code))?; }
            if client_exit_code!=259 { return Ok(()); }
            if unsafe { ConnectNamedPipe(pipe.raw(), null_mut()) } != 0 {
                // NOWAIT can report that the instance is available for connection.
                // Confirm a real peer before proceeding to authentication.
                let mut connected_pid = 0;
                if unsafe { GetNamedPipeClientProcessId(pipe.raw(), &mut connected_pid) } != 0 { break; }
                if Instant::now() >= idle { return Ok(()); }
                pause(idle)?;
                continue;
            }
            match unsafe { GetLastError() } { 535 => break, 536 | 232 => { if Instant::now() >= idle { return Ok(()); } pause(idle)?; }, _ => return Err(last_error()) }
        }
        let mut pid = 0; unsafe { check(GetNamedPipeClientProcessId(pipe.raw(), &mut pid))?; }
        if pid != client_pid { continue; }
        let connected = process(pid)?; same_user(&expected, &identity(connected.raw())?)?;
        let bytes = match receive(&pipe, Instant::now() + Duration::from_secs(10)) { Ok(v)=>v, Err(_)=>continue };
        let request: Request = match serde_json::from_slice(&bytes) { Ok(v)=>v, Err(_)=>continue };
        if request.version != VERSION || request.id <= last_id { continue; }
        last_id = request.id;
        let budget=Duration::from_secs(request.operation.budget_seconds());
        let (tx, rx) = std::sync::mpsc::sync_channel(1); let worker = device.clone(); let usb=usb.clone(); let direct=direct.clone(); let audio=audio.clone(); let logo=logo.clone();
        std::thread::spawn(move || {
            let result = match request.operation { Operation::ThermalStatus=>worker.thermal_status().map(Payload::Thermal), Operation::StartThermal(config)=>worker.start_thermal(config).map(Payload::Thermal), Operation::StopThermal=>worker.stop_thermal().map(Payload::Thermal), Operation::CentralPlugin=>acer_system::central_plugin::query().map(Payload::CentralPlugin).map_err(error), Operation::SetCentralGuard{action}=>acer_system::central_plugin::set_guard(action).map(Payload::CentralPlugin).map_err(error), Operation::Ping=>Ok(Payload::Pong), Operation::Snapshot => worker.snapshot().map(Payload::Snapshot), Operation::Capabilities => worker.capabilities().map(Payload::Capabilities), Operation::Apply(cmd) => worker.apply(cmd).map(Payload::Apply), Operation::OfficialTask(action)=>super::official::execute(action).map(Payload::OfficialTask), Operation::UsbCharge=>Ok(Payload::UsbCharge(usb.query())), Operation::SetUsbCharge(state)=>usb.set(state).map(Payload::UsbCharge), Operation::BootLogo=>logo.query().map(Payload::BootLogo), Operation::PrepareBootLogo(action)=>logo.prepare(action).map(Payload::BootLogo), Operation::CommitBootLogo{plan_id}=>logo.commit(&plan_id).map(Payload::BootLogo), Operation::DiscardBootLogo{plan_id}=>logo.discard(&plan_id).map(Payload::BootLogo), Operation::Audio=>audio.query(&worker).map(Payload::Audio), Operation::SetAudio{preset}=>audio.apply(&worker,preset).map(Payload::Audio), Operation::VendorControls=>Ok(Payload::VendorControls(direct.query())), Operation::SetVendorControl{setting,enabled}=>direct.apply(setting,enabled).map(Payload::VendorControls) };
            let _ = tx.send(result);
        });
        let result = rx.recv_timeout(budget).map_err(|_|error("Device timeout; helper shutting down; outcome unknown"))?;
        let response = serde_json::to_vec(&Response { version: VERSION, id: request.id, result }).map_err(error)?;
        // Per-connection failures do not replay the operation.
        if send(&pipe, &response, Instant::now() + Duration::from_secs(10)).is_ok() {
            let mut acknowledgement = [0u8; 1];
            let _ = transfer(&pipe, &mut acknowledgement, false, Instant::now() + Duration::from_secs(2));
        }
    }
}
