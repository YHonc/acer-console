//! Fixed PawnIO IntelMSR transport. Construction never installs/starts the driver.
//! No public raw setter: writes are reachable only through the supervised trial.
use crate::{cpu_power::Error,msr_power::{self,MsrIo,Snapshot}};
use acer_protocol::rapl_plan::Target;
use sha2::{Digest,Sha256};
use std::{ffi::c_void,marker::PhantomData,path::Path,rc::Rc};

const MODULE_HASH:&str="d6ed85d65ab17a22f813ef98207d6d537155ee2ded5976a21cb48413c9b92e5f";
const EXECUTE:u32=0xA1B22104;
#[link(name="kernel32")]
extern "system" {
    fn CreateFileW(name:*const u16,access:u32,share:u32,security:*const c_void,creation:u32,flags:u32,template:*mut c_void)->*mut c_void;
    fn DeviceIoControl(handle:*mut c_void,code:u32,input:*const c_void,input_len:u32,output:*mut c_void,output_len:u32,returned:*mut u32,overlapped:*mut c_void)->i32;
    fn CloseHandle(handle:*mut c_void)->i32;
}
fn local(op:&str,message:impl Into<String>)->Error{Error{operation:op.into(),code:None,message:message.into()}}
fn os_error(op:&str)->Error{let e=std::io::Error::last_os_error();Error{operation:op.into(),code:e.raw_os_error().map(|v|v as u32),message:e.to_string()}}
fn validate_write(current:u64,expected:u64,value:u64)->Result<(),Error>{
    if current!=expected{return Err(local("write","Register changed before setter; no write issued"));}
    const ALLOWED:u64=0x7fff|(0x7f<<17)|(0x7fff<<32);
    if current&(1<<63)!=0 || (current^value)&!ALLOWED!=0{return Err(local("write","Locked register or non-target bit change"));}
    Ok(())
}
fn packet(write:Option<u64>,register:u64)->Vec<u8>{
    let mut bytes=vec![0; if write.is_some(){48}else{40}];
    let name=if write.is_some(){b"ioctl_write_msr".as_slice()}else{b"ioctl_read_msr".as_slice()};
    bytes[..name.len()].copy_from_slice(name);bytes[32..40].copy_from_slice(&register.to_le_bytes());
    if let Some(value)=write{bytes[40..48].copy_from_slice(&value.to_le_bytes());}bytes
}
struct Transport {handle:*mut c_void,write_enabled:bool,_thread:PhantomData<Rc<()>>}
pub struct PawnMsr {transport:Transport}
impl Transport {
    /// Exact reviewed module bytes only. Caller performs target-machine guards.
    fn open(module:&Path,supervised_write:bool)->Result<Self,Error>{
        let bytes=std::fs::read(module).map_err(|e|local("module",e.to_string()))?;
        if format!("{:x}",Sha256::digest(&bytes))!=MODULE_HASH{return Err(local("module","IntelMSR module hash mismatch"));}
        let path:Vec<u16>="\\\\?\\GLOBALROOT\\Device\\PawnIO\0".encode_utf16().collect();
        let handle=unsafe{CreateFileW(path.as_ptr(),0xC0000000,3,std::ptr::null(),3,0,std::ptr::null_mut())};
        if handle as isize == -1{return Err(os_error("open_pawnio"));}
        let mut io=Self{handle,write_enabled:supervised_write,_thread:PhantomData};
        io.ioctl(0xA1B22084,&bytes,&mut [])?;Ok(io)
    }
    fn ioctl(&mut self,code:u32,input:&[u8],output:&mut [u8])->Result<(),Error>{
        let mut returned=0;
        let ok=unsafe{DeviceIoControl(self.handle,code,input.as_ptr().cast(),input.len() as u32,
            if output.is_empty(){std::ptr::null_mut()}else{output.as_mut_ptr().cast()},output.len() as u32,&mut returned,std::ptr::null_mut())};
        if ok==0{return Err(os_error("PawnIO_IOCTL"));}
        if returned as usize!=output.len(){return Err(local("PawnIO_IOCTL","Unexpected output length"));}Ok(())
    }
    fn register(&mut self,register:u64)->Result<u64,Error>{
        if ![0x606,0x610].contains(&register){return Err(local("read","Register not allowed"));}
        let mut output=[0u8;8];self.ioctl(EXECUTE,&packet(None,register),&mut output)?;Ok(u64::from_le_bytes(output))
    }
}
impl PawnMsr {
    pub fn open(module:&Path,supervised_write:bool)->Result<Self,Error>{Ok(Self{transport:Transport::open(module,supervised_write)?})}
    pub fn snapshot(&mut self)->Result<Snapshot,Error>{self.transport.read()}
    pub fn trial(&mut self,target:Target,persist:impl FnMut(&msr_power::Checkpoint)->Result<(),Error>)->Result<msr_power::Report,Error>{
        if !self.transport.write_enabled{return Err(local("policy","Read-only PawnIO session"));}
        msr_power::trial(&mut self.transport,target,persist)
    }
    pub fn recover(&mut self,checkpoint:&msr_power::Checkpoint)->Result<msr_power::RecoveryReport,Error>{
        if !self.transport.write_enabled{return Err(local("policy","Read-only PawnIO session"));}
        msr_power::recover(&mut self.transport,checkpoint)
    }
}
impl MsrIo for Transport {
    fn read(&mut self)->Result<Snapshot,Error>{Ok(Snapshot{units:self.register(0x606)?,limits:self.register(0x610)?})}
    fn write_limits(&mut self,expected:u64,value:u64)->Result<(),Error>{
        if !self.write_enabled{return Err(local("policy","Read-only PawnIO session"));}
        let current=self.register(0x610)?;
        validate_write(current,expected,value)?;
        self.ioctl(EXECUTE,&packet(Some(value),0x610),&mut [])
    }
}
impl Drop for Transport {fn drop(&mut self){unsafe{CloseHandle(self.handle);}}}
#[cfg(test)]mod tests{
    use super::*;
    #[test]fn stale_full_register_is_rejected_even_for_allowed_fields(){
        let old=0x004284E800DF8320;let target=0x004284E800DF82F8;
        assert!(validate_write(old,old,target).is_ok());
        for bit in [0,17,32,25,63]{assert!(validate_write(old^(1<<bit),old,target).is_err());}
        assert!(validate_write(old,old,target^(1<<15)).is_err());
    }
    #[test]fn exact_module_abi_packets(){
        let read=packet(None,0x606);assert_eq!(read.len(),40);assert_eq!(&read[..15],b"ioctl_read_msr\0");
        assert_eq!(&read[32..],&0x606u64.to_le_bytes());
        let write=packet(Some(0x004284E800DF8320),0x610);assert_eq!(write.len(),48);
        assert_eq!(&write[..16],b"ioctl_write_msr\0");assert_eq!(&write[32..40],&0x610u64.to_le_bytes());
        assert_eq!(&write[40..],&0x004284E800DF8320u64.to_le_bytes());
    }
}
