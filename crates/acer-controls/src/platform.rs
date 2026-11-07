use super::*;
use wmi::{Variant,WMIConnection};
pub(super) struct Backend {connection:WMIConnection}
fn transport_error(e:impl ToString)->DeviceError {
    let message=e.to_string();let lower=message.to_lowercase();
    let kind=if lower.contains("80041003")||lower.contains("80070005")||lower.contains("拒绝访问")||lower.contains("access is denied"){ErrorKind::PermissionDenied}else{ErrorKind::Transport};
    error(kind,message)
}
fn raw(v:Variant)->RawValue {match v {
    Variant::Bool(v)=>RawValue::Bool(v),Variant::UI1(v)=>RawValue::unsigned(v.into()),Variant::UI2(v)=>RawValue::unsigned(v.into()),Variant::UI4(v)=>RawValue::unsigned(v.into()),Variant::UI8(v)=>RawValue::unsigned(v),
    Variant::I1(v)=>RawValue::Signed(v.to_string()),Variant::I2(v)=>RawValue::Signed(v.to_string()),Variant::I4(v)=>RawValue::Signed(v.to_string()),Variant::I8(v)=>RawValue::Signed(v.to_string()),
    Variant::String(v)=>RawValue::Text(v),Variant::Null|Variant::Empty=>RawValue::Null,Variant::Array(v)=>RawValue::Array(v.into_iter().map(raw).collect()),other=>RawValue::Text(format!("{other:?}")),
}}
impl Backend {
    pub fn connect()->Result<Self,DeviceError> {
        let system=WMIConnection::with_namespace_path("ROOT\\CIMV2").map_err(transport_error)?;
        #[derive(Deserialize)]struct Machine{#[serde(rename="Model")]model:String}
        #[derive(Deserialize)]struct Bios{#[serde(rename="SMBIOSBIOSVersion")]bios:String}
        let machines:Vec<Machine>=system.raw_query("SELECT Model FROM Win32_ComputerSystem").map_err(transport_error)?;
        let versions:Vec<Bios>=system.raw_query("SELECT SMBIOSBIOSVersion FROM Win32_BIOS").map_err(transport_error)?;
        if machines.len()!=1||versions.len()!=1||!matches_target(&machines[0].model,&versions[0].bios){return Err(error(ErrorKind::Unsupported,"USB adapter requires PHN16-71 / BIOS V1.18"));}
        Ok(Self{connection:WMIConnection::with_namespace_path("ROOT\\WMI").map_err(transport_error)?})
    }
    fn call(&mut self,method:&str,value:u64)->WriteAttempt {
        let mut attempted=false;
        let mut fields=BTreeMap::new();
        let result=(||->Result<(),DeviceError>{
            #[derive(Deserialize)]struct Instance{#[serde(rename="__PATH")]path:String}
            let rows:Vec<Instance>=self.connection.raw_query("SELECT __PATH FROM APGeAction").map_err(transport_error)?;
            if rows.len()!=1{return Err(error(ErrorKind::Protocol,"Expected exactly one APGeAction instance"));}
            let input=self.connection.get_object("APGeAction").map_err(transport_error)?.get_method(method).map_err(transport_error)?.ok_or_else(||error(ErrorKind::Protocol,"APGeAction method input signature missing"))?.spawn_instance().map_err(transport_error)?;
            let variant=if method=="GetFunction" {Variant::UI4(u32::try_from(value).map_err(transport_error)?)}else{Variant::UI8(value)};
            input.put_property("uiInput",variant).map_err(transport_error)?;
            attempted=true;
            let output=self.connection.exec_method(&rows[0].path,method,Some(&input)).map_err(transport_error)?.ok_or_else(||error(ErrorKind::Protocol,"APGeAction returned no output"))?;
            for name in output.list_properties().map_err(transport_error)?{fields.insert(name.clone(),raw(output.get_property(&name).map_err(transport_error)?));}Ok(())
        })();
        let raw=raw_read(method,value,fields,result.err());
        WriteAttempt{attempted,raw}
    }
}
impl Transport for Backend {
    fn read(&mut self)->RawRead {self.call("GetFunction",4).raw}
    fn write(&mut self,state:UsbState)->WriteAttempt {self.call("SetFunction",encode_usb(state))}
}
