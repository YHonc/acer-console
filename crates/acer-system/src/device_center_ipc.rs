//! Protocol v1 client. Transport is injected, so tests never open devices.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::HashMap, future::Future, sync::{Arc, Mutex}, time::Duration};
use tokio::{io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt}, sync::{mpsc, oneshot}};
use zeroize::{Zeroize, Zeroizing};

pub struct SecretValue(pub Value);
pub fn secret_value(value:Value)->Zeroizing<SecretValue>{Zeroizing::new(SecretValue(value))}
impl Zeroize for SecretValue {
    fn zeroize(&mut self) {
        fn wipe(v:&mut Value) { match v { Value::String(s)=>s.zeroize(), Value::Array(a)=>a.iter_mut().for_each(wipe), Value::Object(o)=>o.values_mut().for_each(wipe), _=>{} } }
        wipe(&mut self.0);
    }
}
fn disconnected()->DcError { let mut e=DcError::new("TIMEOUT","结果未知，请刷新读回");e.details=json!({"reason":"disconnected"});e }

pub const MAX_FRAME: usize = 1024 * 1024;
pub const METHODS: &[&str] = &["device.list", "device.snapshots", "battery.read", "settings.read", "settings.apply", "mouse.dpi.read", "mouse.dpi.apply", "mouse.mapping.read", "mouse.mapping.apply", "keyboard.lighting.read", "keyboard.lighting.apply", "lamp.config.read", "lamp.config.save", "lamp.read", "lamp.apply"];
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Protocol { pub major: u32, pub minor: u32 }
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all="SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    ProtocolMismatch, Unauthorized, UnknownMethod, InvalidParams, ModuleUntrusted,
    DeviceDisconnected, Busy, Timeout, StaleState, ReadbackMismatch, PartialWrite,
    #[serde(other)] Internal,
}
impl ErrorCode {
    fn retryable(self)->bool { matches!(self,Self::Busy|Self::Timeout|Self::DeviceDisconnected) }
}
impl PartialEq<&str> for ErrorCode {
    fn eq(&self,other:&&str)->bool { serde_json::to_value(self).is_ok_and(|v|v==*other) }
}
#[derive(Debug, Clone, Serialize)]
pub struct DcError { pub code: ErrorCode, pub message: String, pub retryable: bool, pub details: Value }
impl<'de> Deserialize<'de> for DcError {
    fn deserialize<D:serde::Deserializer<'de>>(d:D)->Result<Self,D::Error>{
        #[derive(Deserialize)]
        struct Wire { code:ErrorCode, message:String, #[serde(default)] details:Value }
        let w=Wire::deserialize(d)?;
        Ok(Self{code:w.code,message:w.message,retryable:w.code.retryable(),details:w.details})
    }
}
impl DcError {
    pub fn new(code: &str, message: impl ToString) -> Self { let code=serde_json::from_value(json!(code)).unwrap_or(ErrorCode::Internal); Self { code, message: message.to_string(), retryable: code.retryable(), details: Value::Null } }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all="camelCase")]
pub struct HelloAck { pub protocol: Protocol, pub module_id: String, pub module_version: String, pub pid: u32, pub capabilities: Vec<String> }
#[derive(Debug, Clone, Serialize)]
pub struct CallResult {
    pub ok: bool,
    #[serde(skip_serializing_if="Option::is_none")] pub result: Option<Value>,
    #[serde(skip_serializing_if="Option::is_none")] pub error: Option<DcError>,
}
impl<'de> Deserialize<'de> for CallResult {
    fn deserialize<D:serde::Deserializer<'de>>(deserializer:D)->Result<Self,D::Error>{
        use serde::de::Error;
        let value=Value::deserialize(deserializer)?;
        match value["ok"].as_bool(){
            Some(true)=>Ok(Self{ok:true,result:Some(value.get("result").ok_or_else(||D::Error::custom("Missing result"))?.clone()),error:None}),
            Some(false)=>Ok(Self::failure(serde_json::from_value(value["error"].clone()).map_err(D::Error::custom)?)),
            None=>Err(D::Error::custom("Missing ok flag"))
        }
    }
}
impl CallResult {
    pub fn failure(e: DcError) -> Self { Self { ok:false, result:None, error:Some(e) } }
}
pub async fn read_frame<R: AsyncRead + Unpin>(reader: &mut R) -> Result<Value, DcError> {
    let len = reader.read_u32_le().await.map_err(io_error)? as usize;
    if len == 0 || len > MAX_FRAME { return Err(DcError::new("INVALID_PARAMS", "IPC frame length invalid")); }
    let mut bytes = vec![0; len];
    reader.read_exact(&mut bytes).await.map_err(io_error)?;
    serde_json::from_slice(&bytes).map_err(|e| DcError::new("INVALID_PARAMS",e))
}
pub async fn write_frame<W: AsyncWrite + Unpin>(writer: &mut W, value: &Value) -> Result<(), DcError> {
    let bytes = Zeroizing::new(serde_json::to_vec(value).map_err(io_error)?);
    if bytes.len() > MAX_FRAME { return Err(DcError::new("INVALID_PARAMS", "IPC frame too large")); }
    writer.write_u32_le(bytes.len() as u32).await.map_err(io_error)?;
    writer.write_all(&bytes).await.map_err(io_error)?;
    writer.flush().await.map_err(io_error)
}
fn io_error(e: impl ToString) -> DcError { DcError::new("INTERNAL",e) }
pub fn validate_hello(value: Value, pid: u32) -> Result<HelloAck, DcError> {
    if value["type"] == "response" && value["ok"] == false {
        return Err(serde_json::from_value(value["error"].clone()).map_err(io_error)?);
    }
    if value["type"] != "helloAck" { return Err(io_error("Expected helloAck")); }
    let ack: HelloAck = serde_json::from_value(value).map_err(io_error)?;
    if ack.protocol.major != 1 { return Err(DcError::new("PROTOCOL_MISMATCH", "协议版本不兼容")); }
    if ack.module_id != "device-center" || ack.pid != pid { return Err(DcError::new("UNAUTHORIZED", "模块进程身份不一致")); }
    Ok(ack)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    pub fn runtime()->tokio::runtime::Runtime{tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap()}
    pub fn ack()->Value{json!({"type":"helloAck","protocol":{"major":1,"minor":0},"moduleId":"device-center","moduleVersion":"0.1.1","pid":42,"capabilities":METHODS})}
    pub async fn pair()->(Client,tokio::io::DuplexStream,mpsc::UnboundedReceiver<Value>){
        pair_with_capacity(MAX_FRAME*2).await
    }
    pub async fn pair_with_capacity(capacity:usize)->(Client,tokio::io::DuplexStream,mpsc::UnboundedReceiver<Value>){
        let (a,mut b)=tokio::io::duplex(capacity);let (tx,rx)=mpsc::unbounded_channel();
        let server=tokio::spawn(async move{let hello=read_frame(&mut b).await.unwrap();assert_eq!(hello["nonce"],"test-nonce");assert_eq!(hello["clientPid"],std::process::id());write_frame(&mut b,&ack()).await.unwrap();b});
        let client=Client::connect(a,"test-nonce",42,tx).await.unwrap();(client,server.await.unwrap(),rx)
    }
    #[test] fn frames_roundtrip_and_little_endian(){runtime().block_on(async{
        let (mut a,mut b)=tokio::io::duplex(4096);let value=json!({"中文":"值"});
        write_frame(&mut a,&value).await.unwrap();let len=b.read_u32_le().await.unwrap() as usize;assert_eq!(len,serde_json::to_vec(&value).unwrap().len());
        let mut bytes=vec![0;len];b.read_exact(&mut bytes).await.unwrap();assert_eq!(serde_json::from_slice::<Value>(&bytes).unwrap(),value);
        write_frame(&mut a,&value).await.unwrap();assert_eq!(read_frame(&mut b).await.unwrap(),value);
    })}
    #[test] fn invalid_frames(){runtime().block_on(async{
        for length in [0,MAX_FRAME as u32+1] {let (mut a,mut b)=tokio::io::duplex(64);a.write_u32_le(length).await.unwrap();assert_eq!(read_frame(&mut b).await.unwrap_err().code,"INVALID_PARAMS");}
        let (mut a,mut b)=tokio::io::duplex(64);a.write_u32_le(1).await.unwrap();a.write_all(&[255]).await.unwrap();assert_eq!(read_frame(&mut b).await.unwrap_err().code,"INVALID_PARAMS");
    })}
    #[test] fn hello_major_pid_and_module_validation(){
        let mut value=ack();value["protocol"]["major"]=json!(2);assert_eq!(validate_hello(value,42).unwrap_err().code,"PROTOCOL_MISMATCH");
        assert_eq!(validate_hello(ack(),7).unwrap_err().code,"UNAUTHORIZED");
        let mut value=ack();value["moduleId"]=json!("other");assert_eq!(validate_hello(value,42).unwrap_err().code,"UNAUTHORIZED");
        let mut value=ack();value["protocol"]["minor"]=json!(8);assert!(validate_hello(value,42).is_ok());
    }
    #[test] fn fake_server_rejects_unauthorized_and_protocol(){runtime().block_on(async{
        for code in ["UNAUTHORIZED","PROTOCOL_MISMATCH"]{
            let (a,mut b)=tokio::io::duplex(4096);let (tx,_)=mpsc::unbounded_channel();
            let task=tokio::spawn(async move{let _=read_frame(&mut b).await.unwrap();write_frame(&mut b,&json!({"type":"response","id":0,"ok":false,"error":DcError::new(code,"rejected")})).await.unwrap();});
            let e=Client::connect(a,"nonce",42,tx).await.err().unwrap();assert_eq!(e.code,code);task.await.unwrap();
        }
    })}
    #[test] fn whitelist_and_capabilities_intersection(){
        assert!(allowed("device.list",&["device.list".into()]));assert!(!allowed("device.list",&[]));assert!(!allowed("hid.raw",&["hid.raw".into()]));
        for method in METHODS{assert!(allowed(method,&[method.to_string()]));}
    }
    #[test] fn null_result_is_preserved_and_missing_result_rejected(){
        let result:CallResult=serde_json::from_value(json!({"ok":true,"result":null})).unwrap();
        assert_eq!(serde_json::to_value(result).unwrap(),json!({"ok":true,"result":null}));
        assert!(serde_json::from_value::<CallResult>(json!({"ok":true})).is_err());
    }
    #[test] fn ping_pong_uses_request_ids(){runtime().block_on(async{
        let (client,mut server,_)=pair().await;
        let task=tokio::spawn(async move{let ping=read_frame(&mut server).await.unwrap();assert_eq!(ping["type"],"ping");assert_eq!(ping["id"],1);write_frame(&mut server,&json!({"type":"pong","id":ping["id"]})).await.unwrap();});
        client.ping(1000).await.unwrap();task.await.unwrap();
    })}
    #[test] fn oversized_incoming_frame_disconnects_both_halves(){runtime().block_on(async{
        let (_client,mut server,mut events)=pair().await;
        server.write_u32_le(MAX_FRAME as u32+1).await.unwrap();assert_eq!(events.recv().await.unwrap()["type"],"disconnected");
        let mut byte=[0];assert_eq!(tokio::time::timeout(Duration::from_secs(1),server.read(&mut byte)).await.unwrap().unwrap(),0);
    })}
    #[test] fn requests_correlate_out_of_order(){runtime().block_on(async{
        let (client,mut server,_)=pair().await;
        let a=client.clone();let one=tokio::spawn(async move{a.call("device.list",json!({}),1000).await});
        let b=client.clone();let two=tokio::spawn(async move{b.call("device.snapshots",json!({}),1000).await});
        let first=read_frame(&mut server).await.unwrap();let second=read_frame(&mut server).await.unwrap();assert_eq!(first["id"].as_u64().unwrap()+1,second["id"].as_u64().unwrap());
        for req in [second,first]{write_frame(&mut server,&json!({"type":"response","id":req["id"],"ok":true,"result":req["method"]})).await.unwrap();}
        assert_eq!(one.await.unwrap().result.unwrap(),"device.list");assert_eq!(two.await.unwrap().result.unwrap(),"device.snapshots");
    })}
    #[test] fn timeout_cleans_pending_and_rejects_late_response(){runtime().block_on(async{
        let (client,mut server,_)=pair().await;
        let result=client.call("settings.apply",json!({}),15).await;assert_eq!(result.error.unwrap().code,"TIMEOUT");assert!(client.pending.lock().unwrap().is_empty());
        let request=read_frame(&mut server).await.unwrap();write_frame(&mut server,&json!({"type":"response","id":request["id"],"ok":true,"result":{}})).await.unwrap();
        assert_eq!(client.call("raw.hid",json!({}),5).await.error.unwrap().code,"UNKNOWN_METHOD");
    })}
    #[test] fn seq_gap_requests_resync_and_forwards_event(){runtime().block_on(async{
        let (_client,mut server,mut events)=pair().await;
        for seq in [1,3]{write_frame(&mut server,&json!({"type":"event","seq":seq,"name":"battery.updated","deviceId":"fake","data":{}})).await.unwrap();}
        assert_eq!(events.recv().await.unwrap()["seq"],1);assert_eq!(events.recv().await.unwrap()["type"],"resync");assert_eq!(events.recv().await.unwrap()["seq"],3);
    })}
    #[test] fn legacy_gui_events_are_not_forwarded(){runtime().block_on(async{
        let (_client,mut server,mut events)=pair().await;
        for (index,name) in ["request-window-close","request-compact-exit","module.status"].iter().enumerate(){write_frame(&mut server,&json!({"type":"event","seq":index+1,"name":name,"data":{}})).await.unwrap();}
        let event=tokio::time::timeout(Duration::from_secs(1),events.recv()).await.unwrap().unwrap();assert_eq!(event["name"],"module.status");assert_eq!(event["seq"],3);
    })}
    #[test] fn error_codes_roundtrip_and_retryability(){runtime().block_on(async{
        let (client,mut server,_)=pair().await;
        let task=tokio::spawn(async move{let req=read_frame(&mut server).await.unwrap();write_frame(&mut server,&json!({"type":"response","id":req["id"],"ok":false,"error":{"code":"READBACK_MISMATCH","message":"different","retryable":false,"details":{"readback":7}}})).await.unwrap();});
        let e=client.call("settings.apply",json!({}),1000).await.error.unwrap();assert_eq!(e.code,"READBACK_MISMATCH");assert_eq!(e.details["readback"],7);task.await.unwrap();
        for code in ["PROTOCOL_MISMATCH","UNAUTHORIZED","UNKNOWN_METHOD","INVALID_PARAMS","MODULE_UNTRUSTED","DEVICE_DISCONNECTED","BUSY","TIMEOUT","STALE_STATE","READBACK_MISMATCH","PARTIAL_WRITE","INTERNAL"]{
            let e=DcError::new(code,"test");assert_eq!(e.retryable,matches!(code,"BUSY"|"TIMEOUT"|"DEVICE_DISCONNECTED"));let result=CallResult::failure(e);let v=serde_json::to_value(result).unwrap();assert!(v.get("result").is_none());assert_eq!(v["error"]["code"],code);
        }
    })}
    #[test] fn untrusted_wire_retryability_and_unknown_codes(){
        for code in ["PROTOCOL_MISMATCH","UNAUTHORIZED","UNKNOWN_METHOD","INVALID_PARAMS","MODULE_UNTRUSTED","DEVICE_DISCONNECTED","BUSY","TIMEOUT","STALE_STATE","READBACK_MISMATCH","PARTIAL_WRITE","INTERNAL","NEW_CODE"] {
            for supplied in [true,false] {
                let e:DcError=serde_json::from_value(json!({"code":code,"message":"test","retryable":supplied,"details":null})).unwrap();
                assert_eq!(e.retryable,matches!(code,"BUSY"|"TIMEOUT"|"DEVICE_DISCONNECTED"));
                assert_eq!(serde_json::to_value(e.code).unwrap(),if code=="NEW_CODE"{"INTERNAL"}else{code});
            }
        }
    }
    #[test] fn disconnect_marks_apply_and_config_save_unknown(){runtime().block_on(async{
        for method in ["settings.apply","lamp.config.save"] {
            let (client,mut server,_)=pair().await;
            let caller=client.clone();let task=tokio::spawn(async move{caller.call(method,json!({"token":"secret"}),1000).await});
            assert_eq!(read_frame(&mut server).await.unwrap()["method"],method);drop(server);
            let error=task.await.unwrap().error.unwrap();assert_eq!(error.code,"TIMEOUT");assert_eq!(error.details["reason"],"disconnected");assert!(error.retryable);
        }
    })}
    #[test] fn expired_queued_detach_never_reaches_wire(){runtime().block_on(async{
        let (client,mut server,_)=pair_with_capacity(256).await;
        let c=client.clone();let block=tokio::spawn(async move{c.control(json!({"padding":"x".repeat(4096)})).await});
        let length=server.read_u32_le().await.unwrap();
        assert_eq!(client.control_until(json!({"type":"detach"}),std::time::Instant::now()+Duration::from_millis(25)).await.unwrap_err().code,"TIMEOUT");
        let mut payload=vec![0;length as usize];server.read_exact(&mut payload).await.unwrap();assert!(block.await.unwrap().is_ok());
        assert!(tokio::time::timeout(Duration::from_millis(50),read_frame(&mut server)).await.is_err());
        client.control(json!({"type":"ping","id":99})).await.unwrap();assert_eq!(read_frame(&mut server).await.unwrap()["id"],99);
    })}
    #[test] fn explicit_close_resolves_pending_and_rejects_clones(){runtime().block_on(async{
        let (client,mut server,_)=pair().await;
        let caller=client.clone();let task=tokio::spawn(async move{caller.call("settings.apply",json!({}),30000).await});
        read_frame(&mut server).await.unwrap();client.close();assert!(client.is_closed());
        let result=tokio::time::timeout(Duration::from_secs(1),task).await.unwrap().unwrap();
        assert_eq!(result.error.unwrap().details["reason"],"disconnected");
        assert!(client.pending.lock().unwrap().is_empty());
        assert_eq!(client.call("device.list",json!({}),30000).await.error.unwrap().code,"TIMEOUT");
    })}
    #[test] fn secret_value_wipes_nested_params(){
        let mut value=SecretValue(json!({"params":{"token":"secret","nested":["other"]}}));value.zeroize();
        assert_eq!(value.0["params"]["token"],"");assert_eq!(value.0["params"]["nested"][0],"");
    }
}
pub fn allowed(method: &str, capabilities: &[String]) -> bool { METHODS.contains(&method) && capabilities.iter().any(|s|s==method) }
type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<CallResult>>>>;
struct PendingRequest { table:Pending,id:u64 }
impl Drop for PendingRequest {fn drop(&mut self){self.table.lock().unwrap().remove(&self.id);}}
struct Tasks(Vec<tokio::task::JoinHandle<()>>);
impl Drop for Tasks { fn drop(&mut self){for task in &self.0{task.abort();}} }
#[derive(Clone)]
pub struct Client { pub ack: HelloAck, tx: mpsc::Sender<(Zeroizing<SecretValue>,std::time::Instant,oneshot::Sender<Result<(),DcError>>)>, pending: Pending, next: Arc<tokio::sync::Mutex<u64>>, closed:Arc<std::sync::atomic::AtomicBool>, _tasks:Arc<Tasks> }
impl Client {
    pub async fn connect<T>(mut stream:T, nonce:&str, pid:u32, events:mpsc::UnboundedSender<Value>) -> Result<Self,DcError>
    where T:AsyncRead+AsyncWrite+Unpin+Send+'static {
        let ack = tokio::time::timeout(Duration::from_secs(5), async {
            write_frame(&mut stream,&json!({"type":"hello","protocol":{"major":1,"minor":0},"nonce":nonce,"clientPid":std::process::id()})).await?;
            validate_hello(read_frame(&mut stream).await?,pid)
        }).await.map_err(|_|DcError::new("TIMEOUT","hello 超时"))??;
        let (tx,mut rx)=mpsc::channel::<(Zeroizing<SecretValue>,std::time::Instant,oneshot::Sender<Result<(),DcError>>)>(64);
        let pending:Pending=Arc::default();
        let table=pending.clone();
        let (mut reader,mut writer)=tokio::io::split(stream);
        let (writer_stopped,mut stopped)=oneshot::channel::<()>();
        let writer_task=tokio::spawn(async move { let _stopped=writer_stopped;while let Some((value,deadline,done))=rx.recv().await {
            if done.is_closed() || std::time::Instant::now()>=deadline {continue}
            let result=tokio::time::timeout_at(tokio::time::Instant::from_std(deadline),write_frame(&mut writer,&value.0)).await.unwrap_or_else(|_|Err(DcError::new("TIMEOUT","管道写入超时")));
            let failed=result.is_err();let _=done.send(result);if failed{break}
        } });
        let writer_abort=writer_task.abort_handle();
        let reader_task=tokio::spawn(async move {
            let mut seq=0;
            loop {
                let value={
                    let mut read=std::pin::pin!(read_frame(&mut reader));
                    std::future::poll_fn(|cx|{
                        if std::pin::Pin::new(&mut stopped).poll(cx).is_ready(){return std::task::Poll::Ready(None)}
                        read.as_mut().poll(cx).map(Some)
                    }).await
                };
                let Some(Ok(value))=value else{break};
                match value["type"].as_str() {
                    Some("response") => { if let Some(id)=value["id"].as_u64() { if let Some(sender)=table.lock().unwrap().remove(&id) {
                        let result=serde_json::from_value(value).unwrap_or_else(|e|CallResult::failure(io_error(e)));
                        let _=sender.send(result);
                    }}},
                    Some("pong")=>{if let Some(id)=value["id"].as_u64(){if let Some(sender)=table.lock().unwrap().remove(&id){let _=sender.send(CallResult{ok:true,result:Some(Value::Null),error:None});}}},
                    Some("event") => { if let Some(current)=value["seq"].as_u64() {
                        if current != seq+1 { let _=events.send(json!({"type":"resync"})); }
                        seq=current;
                        if matches!(value["name"].as_str(),Some("battery.updated"|"lamp.updated"|"device.changed"|"module.status")){let _=events.send(value);}
                    }},
                    _=>{}
                }
            }
            // Drop both split halves on invalid/oversized input, not only reader.
            writer_abort.abort();
            for (_,sender) in table.lock().unwrap().drain(){let _=sender.send(CallResult::failure(disconnected()));}
            let _=events.send(json!({"type":"disconnected"}));
        });
        Ok(Self{ack,tx,pending,next:Arc::new(tokio::sync::Mutex::new(1)),closed:Arc::default(),_tasks:Arc::new(Tasks(vec![writer_task,reader_task]))})
    }
    pub async fn control(&self,value:Value)->Result<(),DcError>{
        self.control_until(value,std::time::Instant::now()+Duration::from_secs(5)).await
    }
    pub fn is_closed(&self)->bool {self.closed.load(std::sync::atomic::Ordering::SeqCst)||self.tx.is_closed()}
    /// Close the shared transport even when an in-flight Caller holds a clone.
    pub fn close(&self){
        self.closed.store(true,std::sync::atomic::Ordering::SeqCst);
        for task in &self._tasks.0{task.abort();}
        for (_,sender) in self.pending.lock().unwrap().drain(){let _=sender.send(CallResult::failure(disconnected()));}
    }
    pub async fn control_until(&self,value:Value,deadline:std::time::Instant)->Result<(),DcError>{
        let value=Zeroizing::new(SecretValue(value));
        if self.is_closed(){return Err(disconnected())}
        tokio::time::timeout_at(tokio::time::Instant::from_std(deadline),async{
            let (tx,rx)=oneshot::channel();self.tx.send((value,deadline,tx)).await.map_err(|_|disconnected())?;rx.await.map_err(|_|disconnected())?
        }).await.map_err(|_|DcError::new("TIMEOUT","管道写入超时"))?
    }
    pub async fn call(&self,method:&str,params:Value,deadline_ms:u64)->CallResult {
        let mut params=Zeroizing::new(SecretValue(params));
        if !allowed(method,&self.ack.capabilities) { return CallResult::failure(DcError::new("UNKNOWN_METHOD","方法不在白名单或能力列表中")); }
        let timeout_message=if method.ends_with(".apply") || method=="lamp.config.save"{"结果未知，请刷新读回"}else{"请求超时"};
        let mut result=self.exchange(deadline_ms,timeout_message,|id,deadline|{
            let mut frame=json!({"type":"request","id":id,"method":method,"deadlineMs":deadline});
            frame.as_object_mut().unwrap().insert("params".into(),params.0.take());frame
        }).await;
        if (method.ends_with(".apply") || method=="lamp.config.save") && result.error.as_ref().is_some_and(|e|e.code=="INTERNAL") && self.is_closed(){result=CallResult::failure(disconnected());}
        result
    }
    pub async fn ping(&self,deadline_ms:u64)->Result<(),DcError>{
        let result=self.exchange(deadline_ms,"ping 超时",|id,_|json!({"type":"ping","id":id})).await;
        match result.error{Some(e)=>Err(e),None=>Ok(())}
    }
    async fn exchange(&self,deadline_ms:u64,timeout_message:&str,frame:impl FnOnce(u64,u64)->Value)->CallResult{
        let deadline=deadline_ms.clamp(1,30000);
        let outcome=tokio::time::timeout(Duration::from_millis(deadline),async {
            // Hold allocation through frame delivery, so concurrent callers also
            // put monotonically increasing IDs on the wire. Responses stay parallel.
            let mut next=self.next.lock().await;let id=*next;
            *next=next.checked_add(1).ok_or_else(||io_error("Request ID exhausted"))?;
            let (tx,rx)=oneshot::channel();{
                let mut pending=self.pending.lock().unwrap();
                if self.is_closed(){return Err(disconnected())}
                pending.insert(id,tx);
            }
            let _pending=PendingRequest{table:self.pending.clone(),id};
            self.control_until(frame(id,deadline),std::time::Instant::now()+Duration::from_millis(deadline)).await.map_err(|e|if e.code=="INTERNAL"{disconnected()}else{e})?;
            drop(next);
            rx.await.map_err(|_|disconnected())
        }).await;
        match outcome { Ok(Ok(v))=>v, Ok(Err(e))=>CallResult::failure(e), Err(_)=>CallResult::failure(DcError::new("TIMEOUT",timeout_message)) }
    }
}
