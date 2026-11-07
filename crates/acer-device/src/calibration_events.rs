//! Read-only APGeEvent subscription. The COM enumerator remains on one thread;
//! finite Next waits allow shutdown without the wmi crate's infinite iterator.
use std::{sync::{mpsc, Arc, atomic::{AtomicBool, Ordering}}, thread, time::Duration};
use acer_protocol::calibration::{self, Event};

#[derive(Debug, Clone)]
pub struct Notification { pub detail: Vec<u8>, pub event: Event }

pub struct Listener {
    stop: Arc<AtomicBool>,
    events: mpsc::Receiver<Notification>,
    done: mpsc::Receiver<Result<(), String>>,
    worker: Option<thread::JoinHandle<()>>,
    finished: Option<Result<(), String>>,
}
impl Listener {
    /// A ready reply means ExecNotificationQuery succeeded, not that calibration
    /// events have been observed or that hardware calibration has started.
    pub fn start(timeout:Duration)->Result<Self,String> {
        let stop=Arc::new(AtomicBool::new(false)); let flag=stop.clone();
        let (tx,events)=mpsc::sync_channel(64);let (ready_tx,ready)=mpsc::sync_channel(1);let (done_tx,done)=mpsc::sync_channel(1);
        let worker=thread::Builder::new().name("acer-calibration-events".into()).spawn(move|| {
            let result=run(flag,tx,ready_tx);let _=done_tx.send(result);
        }).map_err(|e|e.to_string())?;
        let listener=Self{stop,events,done,worker:Some(worker),finished:None};
        match ready.recv_timeout(timeout) {
            Ok(Ok(()))=>Ok(listener),
            Ok(Err(e))=>{listener.stop.store(true,Ordering::Release);Err(e)},
            Err(e)=>{listener.stop.store(true,Ordering::Release);Err(format!("Event subscription was not confirmed: {e}"))},
        }
    }
    pub fn try_recv(&self)->Result<Notification,mpsc::TryRecvError>{self.events.try_recv()}
    /// True only after the worker's completion reply and successful join attempt.
    /// Its final result may still describe a provider/delivery error.
    pub fn exited(&self)->bool{self.finished.is_some()&&self.worker.is_none()}
    /// A timeout retains the worker handle for a later wait. It never claims
    /// the COM provider has released resources merely because stop was requested.
    pub fn stop(&mut self,timeout:Duration)->Result<(),String> {
        self.stop.store(true,Ordering::Release);
        if let Some(result)=&self.finished{return result.clone()}
        let result=self.done.recv_timeout(timeout).map_err(|e|format!("Event listener exit is unconfirmed: {e}"))?;
        if let Some(worker)=self.worker.take(){if worker.join().is_err(){self.finished=Some(Err("Event listener panicked".into()));return self.finished.clone().unwrap()}}
        self.finished=Some(result.clone());result
    }
}
impl Drop for Listener {fn drop(&mut self){self.stop.store(true,Ordering::Release);}}

fn deliver(detail:Vec<u8>,tx:&mpsc::SyncSender<Notification>)->Result<(),String>{
    if detail.is_empty()||detail.len()>8{return Err("Malformed APGeEvent byte length".into())}
    if detail[0]!=11{return Ok(())}
    let event=calibration::event(&detail).ok_or("Truncated calibration event")?;
    tx.try_send(Notification{detail,event}).map_err(|e|format!("Calibration event delivery failed: {e}"))
}

#[cfg(not(windows))]
fn run(_:Arc<AtomicBool>,_:mpsc::SyncSender<Notification>,ready:mpsc::SyncSender<Result<(),String>>)->Result<(),String>{
    let error="Calibration events require Windows".to_string();let _=ready.send(Err(error.clone()));Err(error)
}
#[cfg(windows)]
fn run(stop:Arc<AtomicBool>,tx:mpsc::SyncSender<Notification>,ready:mpsc::SyncSender<Result<(),String>>)->Result<(),String>{
    let result=(||{
        use windows::{core::BSTR,Win32::{Foundation::RPC_E_TOO_LATE,System::{Com::*,Rpc::{RPC_C_AUTHN_WINNT,RPC_C_AUTHZ_NONE},Wmi::*}}};
        use wmi::{IWbemClassWrapper,Variant};
        struct Apartment;
        impl Drop for Apartment{fn drop(&mut self){unsafe{CoUninitialize()}}}
        unsafe{CoInitializeEx(None,COINIT_MULTITHREADED)}.ok().map_err(|e|e.to_string())?;
        let _apartment=Apartment;
        let security=unsafe{CoInitializeSecurity(None,-1,None,None,RPC_C_AUTHN_LEVEL_DEFAULT,RPC_C_IMP_LEVEL_IMPERSONATE,None,EOAC_NONE,None)};
        if let Err(e)=security{if e.code()!=RPC_E_TOO_LATE{return Err(e.to_string())}}
        let locator:IWbemLocator=unsafe{CoCreateInstance(&WbemLocator,None,CLSCTX_INPROC_SERVER)}.map_err(|e|e.to_string())?;
        let svc=unsafe{locator.ConnectServer(&BSTR::from("ROOT\\WMI"),&BSTR::new(),&BSTR::new(),&BSTR::new(),WBEM_FLAG_CONNECT_USE_MAX_WAIT.0,&BSTR::new(),None)}.map_err(|e|e.to_string())?;
        unsafe{CoSetProxyBlanket(&svc,RPC_C_AUTHN_WINNT,RPC_C_AUTHZ_NONE,None,RPC_C_AUTHN_LEVEL_PKT_PRIVACY,RPC_C_IMP_LEVEL_IMPERSONATE,None,EOAC_NONE)}.map_err(|e|e.to_string())?;
        let enumerator=unsafe{svc.ExecNotificationQuery(&BSTR::from("WQL"),&BSTR::from("SELECT EventDetail FROM APGeEvent"),WBEM_FLAG_FORWARD_ONLY|WBEM_FLAG_RETURN_IMMEDIATELY,None)}.map_err(|e|e.to_string())?;
        if stop.load(Ordering::Acquire){return Ok(())}
        ready.send(Ok(())).map_err(|_|"Subscription owner left".to_string())?;
        while !stop.load(Ordering::Acquire){
            let mut objects=[None];let mut returned=0;
            let result=unsafe{enumerator.Next(250,&mut objects,&mut returned)};
            result.ok().map_err(|e|e.to_string())?;
            if stop.load(Ordering::Acquire){break}
            if returned>1{return Err("WMI returned an invalid object count".into())}
            if returned==1{
                let object=IWbemClassWrapper::new(objects[0].take().ok_or("WMI event object missing")?);
                let Variant::Array(values)=object.get_property("EventDetail").map_err(|e|e.to_string())? else{return Err("APGeEvent is not a byte array".into())};
                let detail=values.into_iter().map(|v|match v{Variant::UI1(b)=>Ok(b),_=>Err("APGeEvent contains a non-byte value".to_string())}).collect::<Result<Vec<_>,_>>()?;
                deliver(detail,&tx)?;
            }
            if result.0==WBEM_S_FALSE.0{return Err("WMI event subscription ended unexpectedly".into())}
            if result.0!=0&&result.0!=WBEM_S_TIMEDOUT.0{return Err(format!("Unexpected WMI event status: {:#x}",result.0))}
            if returned==0&&result.0==0{return Err("WMI returned success without an event".into())}
        }
        // Release enumerator/services before the apartment and before done reply.
        Ok(())
    })();
    if let Err(e)=&result{let _=ready.try_send(Err(e.clone()));}result
}

#[cfg(test)]mod tests{
    use super::*;
    #[test]fn queue_preserves_delivery_order_and_unknown_events(){
        let(tx,rx)=mpsc::sync_channel(3);deliver(vec![10,1],&tx).unwrap();
        for b in [9,1,0]{deliver(vec![11,b],&tx).unwrap()}
        assert_eq!(rx.recv().unwrap().event,Event::Unknown(9));assert_eq!(rx.recv().unwrap().event,Event::Completed);assert_eq!(rx.recv().unwrap().event,Event::Interrupted);
    }
    #[test]fn overflow_and_malformed_events_are_errors_not_silent_loss(){
        let(tx,_rx)=mpsc::sync_channel(1);deliver(vec![11,1],&tx).unwrap();assert!(deliver(vec![11,0],&tx).is_err());
        assert!(deliver(vec![11],&tx).is_err());assert!(deliver(vec![11;9],&tx).is_err());
    }
}
