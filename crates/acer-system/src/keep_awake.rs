//! Thread-bound execution-state lease. Acquire only for an explicit running task.
use crate::{error,SystemError};
use std::{marker::PhantomData,rc::Rc};
const CONTINUOUS:u32=0x80000000;
const CALIBRATION:u32=CONTINUOUS|1|2;
trait Api {fn set(&mut self,flags:u32)->Result<u32,SystemError>;}
struct Lease<A:Api>{api:A,previous:u32,active:bool,_thread:PhantomData<Rc<()>>}
impl<A:Api> Lease<A>{
    fn acquire(mut api:A)->Result<Self,SystemError>{let previous=api.set(CALIBRATION)?;Ok(Self{api,previous,active:true,_thread:PhantomData})}
    fn release(&mut self)->Result<(),SystemError>{
        if self.active{self.api.set(self.previous|CONTINUOUS)?;self.active=false;}Ok(())
    }
}
impl<A:Api> Drop for Lease<A>{fn drop(&mut self){let _=self.release();}}
struct Native;
impl Api for Native{
    fn set(&mut self,flags:u32)->Result<u32,SystemError>{
        #[cfg(windows)]{
            #[link(name="kernel32")]extern "system"{fn SetThreadExecutionState(flags:u32)->u32;}
            let previous=unsafe{SetThreadExecutionState(flags)};
            if previous==0{Err(error("SetThreadExecutionState failed"))}else{Ok(previous)}
        }
        #[cfg(not(windows))]{let _=flags;Err(error("Execution-state lease requires Windows"))}
    }
}
/// !Send and !Sync: release must run on the thread which acquired the request.
/// Explicit release reports errors; Drop is only the final best-effort fallback.
pub struct KeepAwake{lease:Lease<Native>}
impl KeepAwake{
    pub fn acquire()->Result<Self,SystemError>{Ok(Self{lease:Lease::acquire(Native)?})}
    pub fn release(&mut self)->Result<(),SystemError>{self.lease.release()}
}
#[cfg(test)]mod tests{
    use super::*;use std::cell::RefCell;
    struct Fake{calls:Rc<RefCell<Vec<u32>>>,fail_restore:bool}
    impl Api for Fake{fn set(&mut self,flags:u32)->Result<u32,SystemError>{self.calls.borrow_mut().push(flags);if self.fail_restore&&flags!=CALIBRATION{self.fail_restore=false;return Err(error("restore failure"))}Ok(CONTINUOUS|1)}}
    #[test]fn restores_previous_request_and_explicit_release_is_idempotent(){
        let calls=Rc::new(RefCell::new(vec![]));let mut lease=Lease::acquire(Fake{calls:calls.clone(),fail_restore:false}).unwrap();
        lease.release().unwrap();lease.release().unwrap();drop(lease);assert_eq!(*calls.borrow(),vec![CALIBRATION,CONTINUOUS|1]);
    }
    #[test]fn release_failure_remains_active_for_retry_and_drop_cleans_up(){
        let calls=Rc::new(RefCell::new(vec![]));let mut lease=Lease::acquire(Fake{calls:calls.clone(),fail_restore:true}).unwrap();
        assert!(lease.release().is_err());drop(lease);assert_eq!(*calls.borrow(),vec![CALIBRATION,CONTINUOUS|1,CONTINUOUS|1]);
    }
}
