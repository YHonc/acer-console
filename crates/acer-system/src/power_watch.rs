//! Native, read-only power notifications. No WMI, elevation, or sleep requests.
use super::{error, SystemError};
use serde::Serialize;
use std::{sync::{mpsc, Arc, atomic::{AtomicBool, AtomicU64, Ordering}}, time::Duration};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerEvent { PowerChanged, Resumed }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct PowerStatus {
    pub ac_online: Option<bool>, pub battery_percent: Option<u8>,
    pub charging: Option<bool>, pub battery_present: Option<bool>,
    pub battery_saver: Option<bool>,
}
fn decode_status(ac:u8,flags:u8,percent:u8,saver:u8)->PowerStatus {
    let present=if flags==255 {None} else {Some(flags&128==0)};
    PowerStatus {
        ac_online:match ac {0=>Some(false),1=>Some(true),_=>None},
        battery_percent:if percent<=100 && present!=Some(false) {Some(percent)} else {None},
        charging:if flags==255 || present==Some(false) {None} else {Some(flags&8!=0)},
        battery_present:present, battery_saver:match saver {0=>Some(false),1=>Some(true),_=>None},
    }
}
/// Read-only Win32 query. Unknown byte values remain None.
pub fn get_power_status()->Result<PowerStatus,SystemError> {
    #[cfg(windows)] { native::status() }
    #[cfg(not(windows))] { Err(error("Power status requires Windows")) }
}
struct Sink {
    tx:mpsc::SyncSender<PowerEvent>, dropped:Arc<AtomicU64>,
    resume_pending:Arc<AtomicBool>, power_pending:Arc<AtomicBool>,
}
impl Sink {
    fn emit(&self,event:PowerEvent) {
        if let Err(mpsc::TrySendError::Full(_))=self.tx.try_send(event) {
            self.dropped.fetch_add(1,Ordering::Relaxed);
            match event {PowerEvent::Resumed=>&self.resume_pending,PowerEvent::PowerChanged=>&self.power_pending}.store(true,Ordering::Release);
        }
    }
}
pub struct PowerWatcher {
    rx:mpsc::Receiver<PowerEvent>, dropped:Arc<AtomicU64>,
    resume_pending:Arc<AtomicBool>,power_pending:Arc<AtomicBool>,
    #[cfg(windows)] thread:Option<std::thread::JoinHandle<Result<(),SystemError>>>,
    #[cfg(windows)] thread_id:u32,
}
impl PowerWatcher {
    /// Creates one invisible native window and an event queue of 1..=1024 entries.
    pub fn start(capacity:usize)->Result<Self,SystemError> {
        if !(1..=1024).contains(&capacity) {return Err(error("Power queue capacity must be 1..=1024"));}
        #[cfg(windows)] {
            let(tx,rx)=mpsc::sync_channel(capacity);
            let dropped=Arc::new(AtomicU64::new(0));let resume_pending=Arc::new(AtomicBool::new(false));let power_pending=Arc::new(AtomicBool::new(false));
            let sink=Sink{tx,dropped:dropped.clone(),resume_pending:resume_pending.clone(),power_pending:power_pending.clone()};
            let(ready_tx,ready_rx)=mpsc::sync_channel(1);
            let thread=std::thread::Builder::new().name("acer-power-events".into()).spawn(move||native::run(sink,ready_tx)).map_err(error)?;
            let thread_id=ready_rx.recv_timeout(Duration::from_secs(5)).map_err(error)??;
            Ok(Self{rx,dropped,resume_pending,power_pending,thread:Some(thread),thread_id})
        }
        #[cfg(not(windows))] {Err(error("Power notifications require Windows"))}
    }
    fn pending(&self)->Option<PowerEvent> {
        if self.resume_pending.swap(false,Ordering::AcqRel) {Some(PowerEvent::Resumed)}
        else if self.power_pending.swap(false,Ordering::AcqRel) {Some(PowerEvent::PowerChanged)} else {None}
    }
    pub fn recv_timeout(&self,timeout:Duration)->Result<PowerEvent,mpsc::RecvTimeoutError> {
        if let Some(event)=self.pending(){return Ok(event)} self.rx.recv_timeout(timeout)
    }
    pub fn try_recv(&self)->Result<PowerEvent,mpsc::TryRecvError> {
        if let Some(event)=self.pending(){return Ok(event)} self.rx.try_recv()
    }
    /// Full-queue occurrences, not the number of unique real AC/DC transitions.
    pub fn dropped_events(&self)->u64 {self.dropped.load(Ordering::Relaxed)}
    pub fn is_running(&self)->bool {
        #[cfg(windows)] {self.thread.as_ref().is_some_and(|t|!t.is_finished())}
        #[cfg(not(windows))] {false}
    }
    /// Unregister notifications, destroy the window, and join its thread.
    pub fn shutdown(&mut self)->Result<(),SystemError> {
        #[cfg(windows)] {
            if let Some(thread)=self.thread.take() {
                if !thread.is_finished() {
                    if let Err(err)=native::stop(self.thread_id) {self.thread=Some(thread);return Err(err)}
                }
                return thread.join().map_err(|_|error("Power notification thread panicked"))?;
            }
        }
        Ok(())
    }
}
impl Drop for PowerWatcher {fn drop(&mut self){let _=self.shutdown();}}

#[cfg(windows)] mod native {
    use super::*;
    use std::{ffi::c_void,mem::zeroed,ptr::{null,null_mut}};
    type Handle=*mut c_void;
    #[repr(C)] #[derive(Clone,Copy,PartialEq,Eq)] struct Guid {a:u32,b:u16,c:u16,d:[u8;8]}
    const AC_GUID:Guid=Guid{a:0x5d3e9a59,b:0xe9d5,c:0x4b00,d:[0xa6,0xbd,0xff,0x34,0xff,0x51,0x65,0x48]};
    #[repr(C)] struct Setting {guid:Guid,length:u32,data:[u8;1]}
    #[repr(C)] struct RawStatus {ac:u8,flags:u8,percent:u8,saver:u8,lifetime:u32,full_lifetime:u32}
    #[repr(C)] struct Point {x:i32,y:i32}
    #[repr(C)] struct Message {hwnd:Handle,message:u32,wparam:usize,lparam:isize,time:u32,point:Point}
    #[repr(C)] struct Class {style:u32,proc:Option<unsafe extern "system" fn(Handle,u32,usize,isize)->isize>,class_extra:i32,window_extra:i32,instance:Handle,icon:Handle,cursor:Handle,background:Handle,menu:*const u16,name:*const u16}
    #[repr(C)] struct Create {param:*mut c_void,instance:Handle,menu:Handle,parent:Handle,height:i32,width:i32,y:i32,x:i32,style:i32,name:*const u16,class:*const u16,extended:u32}
    #[link(name="kernel32")] unsafe extern "system" {
        fn GetSystemPowerStatus(status:*mut RawStatus)->i32;
        fn GetModuleHandleW(name:*const u16)->Handle;
        fn GetCurrentThreadId()->u32;
    }
    #[link(name="user32")] unsafe extern "system" {
        fn RegisterClassW(class:*const Class)->u16;
        fn UnregisterClassW(name:*const u16,instance:Handle)->i32;
        fn CreateWindowExW(extended:u32,class:*const u16,title:*const u16,style:u32,x:i32,y:i32,width:i32,height:i32,parent:Handle,menu:Handle,instance:Handle,param:*mut c_void)->Handle;
        fn DestroyWindow(hwnd:Handle)->i32;
        fn GetMessageW(message:*mut Message,hwnd:Handle,min:u32,max:u32)->i32;
        fn TranslateMessage(message:*const Message)->i32;
        fn DispatchMessageW(message:*const Message)->isize;
        fn DefWindowProcW(hwnd:Handle,message:u32,wparam:usize,lparam:isize)->isize;
        fn SetWindowLongPtrW(hwnd:Handle,index:i32,value:isize)->isize;
        fn GetWindowLongPtrW(hwnd:Handle,index:i32)->isize;
        fn PostThreadMessageW(thread:u32,message:u32,wparam:usize,lparam:isize)->i32;
        fn RegisterPowerSettingNotification(recipient:Handle,guid:*const Guid,flags:u32)->Handle;
        fn UnregisterPowerSettingNotification(registration:Handle)->i32;
    }
    fn last()->SystemError {error(std::io::Error::last_os_error())}
    pub(super) fn status()->Result<PowerStatus,SystemError> {
        let mut raw:RawStatus=unsafe{zeroed()};
        if unsafe{GetSystemPowerStatus(&mut raw)}==0 {return Err(last())}
        Ok(decode_status(raw.ac,raw.flags,raw.percent,raw.saver))
    }
    fn classify(code:usize)->Option<PowerEvent> {
        match code {0x0a=>Some(PowerEvent::PowerChanged),0x12=>Some(PowerEvent::Resumed),_=>None}
    }
    unsafe extern "system" fn procedure(hwnd:Handle,message:u32,wparam:usize,lparam:isize)->isize {
        if message==0x81 { // WM_NCCREATE; lpCreateParams belongs to this thread until teardown.
            let create=&*(lparam as *const Create);
            SetWindowLongPtrW(hwnd,-21,create.param as isize);
        }
        if message==0x218 {
            let sink=GetWindowLongPtrW(hwnd,-21) as *const Sink;
            if !sink.is_null() {
                if let Some(event)=classify(wparam) {(*sink).emit(event);return 1}
                if wparam==0x8013 && lparam!=0 {
                    let setting=&*(lparam as *const Setting);
                    if setting.guid==AC_GUID && setting.length==4 {(*sink).emit(PowerEvent::PowerChanged);return 1}
                }
            }
        }
        if message==0x82 {SetWindowLongPtrW(hwnd,-21,0);} // WM_NCDESTROY
        DefWindowProcW(hwnd,message,wparam,lparam)
    }
    pub(super) fn stop(id:u32)->Result<(),SystemError> {
        if unsafe{PostThreadMessageW(id,0x12,0,0)}==0 {return Err(last())} Ok(()) // WM_QUIT
    }
    pub(super) fn run(sink:Sink,ready:mpsc::SyncSender<Result<u32,SystemError>>)->Result<(),SystemError> {
        let mut ready=Some(ready);
        let result=run_inner(sink,&mut ready);
        if let Some(ready)=ready {let _=ready.send(Err(result.as_ref().err().cloned().unwrap_or_else(||error("Power registration stopped"))));}
        result
    }
    fn run_inner(sink:Sink,ready:&mut Option<mpsc::SyncSender<Result<u32,SystemError>>>)->Result<(),SystemError> {
        let sink=Box::new(sink);
        let instance=unsafe{GetModuleHandleW(null())}; if instance.is_null(){return Err(last())}
        let name:Vec<u16>=format!("AcerPowerWatch-{}-{}",std::process::id(),unsafe{GetCurrentThreadId()}).encode_utf16().chain(Some(0)).collect();
        let class=Class{style:0,proc:Some(procedure),class_extra:0,window_extra:0,instance,icon:null_mut(),cursor:null_mut(),background:null_mut(),menu:null(),name:name.as_ptr()};
        if unsafe{RegisterClassW(&class)}==0 {return Err(last())}
        struct Registered<'a>{name:&'a [u16],instance:Handle}
        impl Drop for Registered<'_>{fn drop(&mut self){unsafe{UnregisterClassW(self.name.as_ptr(),self.instance);}}}
        let _class=Registered{name:&name,instance};
        let window=unsafe{CreateWindowExW(0,name.as_ptr(),name.as_ptr(),0,0,0,0,0,null_mut(),null_mut(),instance,(&*sink as *const Sink).cast_mut().cast())};
        if window.is_null(){return Err(last())}
        struct Window(Handle);impl Drop for Window{fn drop(&mut self){unsafe{DestroyWindow(self.0);}}}
        let _window=Window(window); // Hidden top-level window; message-only windows miss broadcasts.
        let registration=unsafe{RegisterPowerSettingNotification(window,&AC_GUID,0)};
        if registration.is_null(){return Err(last())}
        struct Registration(Handle);impl Drop for Registration{fn drop(&mut self){unsafe{UnregisterPowerSettingNotification(self.0);}}}
        let _registration=Registration(registration);
        // Ensure a message queue exists before publishing the thread id.
        if ready.take().unwrap().send(Ok(unsafe{GetCurrentThreadId()})).is_err(){return Ok(())}
        let mut message:Message=unsafe{zeroed()};
        loop {
            let result=unsafe{GetMessageW(&mut message,null_mut(),0,0)};
            if result==-1 {return Err(last())} if result==0 {return Ok(())}
            unsafe{TranslateMessage(&message);DispatchMessageW(&message);}
        }
    }
    #[cfg(test)] mod tests {
        use super::*;
        #[test] fn classify_resume_once_and_status_change() {
            assert_eq!(classify(0x12),Some(PowerEvent::Resumed));
            assert_eq!(classify(7),None); // User-input resume follows automatic resume.
            assert_eq!(classify(0x0a),Some(PowerEvent::PowerChanged));assert_eq!(classify(4),None);
            assert_eq!(std::mem::size_of::<RawStatus>(),12);
        }
    }
}
#[cfg(test)] mod tests {
    use super::*;
    #[test] fn decode_unknown_and_missing_battery() {
        let s=decode_status(255,255,255,7);assert_eq!(s.ac_online,None);assert_eq!(s.battery_percent,None);assert_eq!(s.charging,None);
        let s=decode_status(1,128,0,0);assert_eq!(s.battery_present,Some(false));assert_eq!(s.battery_percent,None);
        let s=decode_status(0,8,47,1);assert_eq!(s.ac_online,Some(false));assert_eq!(s.battery_percent,Some(47));assert_eq!(s.charging,Some(true));
    }
    #[test] fn full_queue_keeps_resume_pending_without_blocking() {
        let(tx,rx)=mpsc::sync_channel(1);let dropped=Arc::new(AtomicU64::new(0));let resume=Arc::new(AtomicBool::new(false));let power=Arc::new(AtomicBool::new(false));
        let sink=Sink{tx,dropped:dropped.clone(),resume_pending:resume.clone(),power_pending:power};
        sink.emit(PowerEvent::PowerChanged);sink.emit(PowerEvent::Resumed);
        assert_eq!(dropped.load(Ordering::Relaxed),1);assert!(resume.load(Ordering::Acquire));assert_eq!(rx.try_recv().unwrap(),PowerEvent::PowerChanged);
    }
}
