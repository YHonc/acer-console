use super::*;
use wmi::{Variant,WMIConnection};

pub(super) struct Backend { connection:WMIConnection }
fn transport_error(e:impl ToString) -> DeviceError {
    let message=e.to_string(); let lower=message.to_lowercase();
    let kind=if ["80041003","80070005","access is denied","拒绝访问"].iter().any(|s|lower.contains(s)) { ErrorKind::PermissionDenied } else { ErrorKind::Transport };
    error(kind,message)
}
fn raw(value:Variant) -> RawValue { match value {
    Variant::Bool(v)=>RawValue::Bool(v), Variant::UI1(v)=>RawValue::unsigned(v.into()),
    Variant::UI2(v)=>RawValue::unsigned(v.into()), Variant::UI4(v)=>RawValue::unsigned(v.into()), Variant::UI8(v)=>RawValue::unsigned(v),
    Variant::I1(v)=>RawValue::Signed(v.to_string()), Variant::I2(v)=>RawValue::Signed(v.to_string()),
    Variant::I4(v)=>RawValue::Signed(v.to_string()), Variant::I8(v)=>RawValue::Signed(v.to_string()),
    Variant::String(v)=>RawValue::Text(v), Variant::Null|Variant::Empty=>RawValue::Null,
    Variant::Array(values)=>RawValue::Array(values.into_iter().map(raw).collect()), other=>RawValue::Text(format!("{other:?}")),
} }
impl Backend {
    pub fn connect() -> Result<(Self,Identity),DeviceError> {
        let system=WMIConnection::with_namespace_path("ROOT\\CIMV2").map_err(transport_error)?;
        #[derive(Deserialize)] struct Machine { #[serde(rename="Model")] model:String }
        #[derive(Deserialize)] struct Bios { #[serde(rename="SMBIOSBIOSVersion")] bios:String }
        let models:Vec<Machine>=system.raw_query("SELECT Model FROM Win32_ComputerSystem").map_err(transport_error)?;
        let versions:Vec<Bios>=system.raw_query("SELECT SMBIOSBIOSVersion FROM Win32_BIOS").map_err(transport_error)?;
        if models.len()!=1 || versions.len()!=1 { return Err(error(ErrorKind::Protocol,"Expected one machine and BIOS identity")); }
        let identity=Identity { model:models.into_iter().next().unwrap().model,bios:versions.into_iter().next().unwrap().bios };
        if !crate::matches_target(&identity.model,&identity.bios) { return Err(error(ErrorKind::Unsupported,"Direct settings require PHN16-71 / BIOS V1.18")); }
        Ok((Self { connection:WMIConnection::with_namespace_path("ROOT\\WMI").map_err(transport_error)? },identity))
    }
}
impl Transport for Backend {
    fn call(&mut self,call:Call,write:bool) -> WriteAttempt {
        let mut attempted=false; let mut fields=BTreeMap::new();
        let result=(|| -> Result<(),DeviceError> {
            // Both branches are fixed literals; no caller-provided class/query/method.
            let query=match call.class { "AcerGamingFunction"=>"SELECT __PATH FROM AcerGamingFunction", "APGeAction"=>"SELECT __PATH FROM APGeAction", _=>return Err(error(ErrorKind::InvalidArgument,"Unknown fixed direct class")) };
            #[derive(Deserialize)] struct Instance { #[serde(rename="__PATH")] path:String }
            let instances:Vec<Instance>=self.connection.raw_query(query).map_err(transport_error)?;
            if instances.len()!=1 { return Err(error(ErrorKind::Protocol,"Expected one fixed WMI class instance")); }
            let input=self.connection.get_object(call.class).map_err(transport_error)?.get_method(call.method).map_err(transport_error)?
                .ok_or_else(||error(ErrorKind::Protocol,"Fixed WMI method signature missing"))?.spawn_instance().map_err(transport_error)?;
            let value=if write { Variant::UI8(call.value) } else { Variant::UI4(u32::try_from(call.value).map_err(transport_error)?) };
            input.put_property(call.parameter,value).map_err(transport_error)?;
            // Mark before dispatch: a COM error can arrive after firmware changed.
            attempted=true;
            let output=self.connection.exec_method(&instances[0].path,call.method,Some(&input)).map_err(transport_error)?
                .ok_or_else(||error(ErrorKind::Protocol,"Fixed WMI method returned no output"))?;
            for name in output.list_properties().map_err(transport_error)? { fields.insert(name.clone(),raw(output.get_property(&name).map_err(transport_error)?)); }
            Ok(())
        })();
        WriteAttempt { attempted,raw:call.raw(fields,result.err()) }
    }
}
