//! Fixed PredatorSense controls. This adapter requires the official local Agent service.
use crate::{error,DeviceError};
use serde::{Deserialize,Serialize};
use serde_json::{json,Value};
use std::{io::{Read,Write},net::{SocketAddr,TcpStream},time::{Duration,Instant}};

#[derive(Debug,Clone,Copy,PartialEq,Eq,Serialize,Deserialize)]
#[serde(rename_all="snake_case")]
pub enum VendorSetting { LcdOverdrive, BootSound, StickyKey, BacklightTimeout }
impl VendorSetting {
    fn function(self)->&'static str {match self{Self::LcdOverdrive=>"LCD_OVERDRIVE",Self::BootSound=>"BOOT_SOUND",Self::StickyKey=>"STICKY_KEY",Self::BacklightTimeout=>"BACKLIGHT_30S"}}
}
const SETTINGS:[VendorSetting;4]=[VendorSetting::LcdOverdrive,VendorSetting::BootSound,VendorSetting::StickyKey,VendorSetting::BacklightTimeout];

fn decode_response(value:&Value,setting:VendorSetting)->Result<(),DeviceError>{
    if value["request"].as_str()!=Some(setting.function()){return Err(error("Official service response does not match requested setting"))}
    if value["result"].as_str()!=Some("0"){return Err(error(format!("Official service rejected operation: {}",value["result"])))}
    Ok(())
}
fn decode_state(value:&Value,setting:VendorSetting)->Result<bool,DeviceError>{
    decode_response(value,setting)?;
    match value["data"]["status"].as_u64(){Some(0)=>Ok(false),Some(1)=>Ok(true),_=>Err(error("Official service returned unknown setting state"))}
}
fn request(packet_id:u32,setting:VendorSetting,enabled:Option<bool>)->Result<Value,DeviceError>{
    let address=SocketAddr::from(([127,0,0,1],46933));
    let deadline=Instant::now()+Duration::from_secs(3);
    let mut stream=TcpStream::connect_timeout(&address,Duration::from_secs(1)).map_err(error)?;
    stream.set_write_timeout(Some(Duration::from_secs(1))).map_err(error)?;
    let mut payload=json!({"Function":setting.function()});
    if let Some(enabled)=enabled{payload["Parameter"]=json!({"status":u8::from(enabled)});}
    let mut bytes=b"ACER".to_vec();bytes.extend_from_slice(&packet_id.to_le_bytes());bytes.extend(serde_json::to_vec(&payload).map_err(error)?);
    stream.write_all(&bytes).map_err(error)?;
    let mut response=Vec::new();let mut chunk=[0u8;4096];
    loop{
        let remaining=deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero(){return Err(error("Official service response timed out; operation outcome may be unknown"))}
        stream.set_read_timeout(Some(remaining)).map_err(error)?;
        let count=stream.read(&mut chunk).map_err(error)?;
        if count==0{return Err(error("Official service disconnected before a complete response"))}
        response.extend_from_slice(&chunk[..count]);
        if response.len()>65536{return Err(error("Official service response exceeds fixed limit"))}
        match serde_json::from_slice::<Value>(&response){Ok(value)=>return Ok(value),Err(e) if e.is_eof()=>{},Err(e)=>return Err(error(e))}
    }
}
fn get(setting:VendorSetting)->Result<(bool,Value),DeviceError>{let raw=request(20,setting,None)?;Ok((decode_state(&raw,setting)?,raw))}
fn write(setting:VendorSetting,enabled:bool)->Result<Value,DeviceError>{
    let acknowledgement=request(100,setting,Some(enabled))?;
    decode_response(&acknowledgement,setting)?;
    let (actual,readback)=get(setting)?;
    if actual!=enabled{return Err(error("Official service readback did not match the requested setting"))}
    Ok(json!({"acknowledgement":acknowledgement,"readback":readback,"enabled":actual}))
}
pub(crate) fn query(writable:bool)->Value{
    let items:Vec<_>=SETTINGS.into_iter().map(|setting|match get(setting){
        Ok((enabled,raw))=>json!({"setting":setting,"enabled":enabled,"raw":raw,"error":null,"writable":writable}),
        Err(e)=>json!({"setting":setting,"enabled":null,"error":e.message,"writable":false}),
    }).collect();
    json!({"backend":"official_service","dependency":"AcerAgentService on 127.0.0.1:46933","items":items})
}
pub(crate) fn validate(setting:VendorSetting)->Result<Value,DeviceError>{
    let (original,before)=get(setting)?;
    let trial=write(setting,!original);
    // A timed-out SET may already have changed the setting. Always restore after attempting it.
    let restored=write(setting,original);
    let after=get(setting);
    let success=trial.is_ok()&&restored.is_ok()&&after.as_ref().is_ok_and(|(enabled,_)|*enabled==original);
    Ok(json!({"backend":"official_service","setting":setting,"before":before,"target":!original,"trial":trial,"restored":restored,"after":after,"success":success}))
}

#[cfg(test)]mod tests{
    use super::*;
    #[test]fn response_checks_function_string_result_and_exact_boolean_encoding(){
        let good=json!({"request":"BOOT_SOUND","result":"0","data":{"status":1}});
        assert_eq!(decode_state(&good,VendorSetting::BootSound).unwrap(),true);
        assert!(decode_state(&good,VendorSetting::LcdOverdrive).is_err());
        for state in [json!(null),json!(true),json!("1"),json!(2),json!(-1)]{let mut raw=good.clone();raw["data"]["status"]=state;assert!(decode_state(&raw,VendorSetting::BootSound).is_err());}
        let mut raw=good;raw["result"]=json!(0);assert!(decode_state(&raw,VendorSetting::BootSound).is_err());
    }
    #[test]fn only_fixed_setting_names_deserialize(){
        assert!(serde_json::from_str::<VendorSetting>(r#""boot_sound""#).is_ok());
        assert!(serde_json::from_str::<VendorSetting>(r#""SET_DEVICE_DATA""#).is_err());
    }
}
