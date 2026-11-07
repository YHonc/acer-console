//! A single owning thread for COM. Hardware writes are opt-in at the native layer.
pub use acer_protocol::DeviceError;
pub mod calibration_events;
pub mod gpu_power;
pub mod cpu_power;
pub mod msr_power;
#[cfg(all(windows,target_arch="x86_64"))]
pub mod pawn_msr;
#[cfg(all(windows, target_arch = "x86_64"))]
mod ipf_abi;
#[cfg(all(windows, target_arch = "x86_64"))]
mod ipf_callbacks;
#[cfg(all(windows, target_arch = "x86_64"))]
pub mod ipf_session;
mod fan_recovery;
mod thermal_runtime;
pub use thermal_runtime::{ThermalConfig, ThermalStatus};
#[cfg(windows)] pub mod calibration_hardware;
use acer_protocol::*;
use std::sync::mpsc;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WritePolicy { #[default] ReadOnly,
    /// Target-specific validated commands only. No webview policy override.
    Verified,
    /// For a supervised validation session only; never accepted from webview JSON.
    AllowUnverified }
pub fn verified_fan_target(model:&str,bios:&str)->bool{matches!(model,"PHN16-71"|"Predator PHN16-71")&&bios=="V1.18"}
pub fn policy_allows(policy:WritePolicy,model:&str,bios:&str,command:&Command)->bool{
    if !verified_fan_target(model,bios){return false;}
    match policy{WritePolicy::ReadOnly=>false,WritePolicy::Verified=>match command{Command::SetFanMode{mode:FanMode::Auto|FanMode::Max}|Command::SetProfile{profile:Profile::Quiet|Profile::Balanced|Profile::Performance|Profile::Turbo}|Command::SetBatteryCare{..}|Command::RestoreLighting{}=>true,Command::SetLighting{lighting}=>lighting.brightness<=100,Command::SetLightingAnimation{lighting}=>valid_animation(lighting),Command::SetFanSpeeds{cpu_percent,gpu_percent}=>custom_percent_allowed(*cpu_percent,*gpu_percent),_=>false},WritePolicy::AllowUnverified=>true}
}
#[derive(Clone)]
pub struct Device { sender: mpsc::Sender<Request> }
enum Request { StartThermal(ThermalConfig,mpsc::Sender<Result<ThermalStatus,DeviceError>>), StopThermal(mpsc::Sender<Result<ThermalStatus,DeviceError>>), ThermalStatus(mpsc::Sender<Result<ThermalStatus,DeviceError>>), SaveLightingCheckpoint(mpsc::Sender<Result<ApplyReport,DeviceError>>), Snapshot(mpsc::Sender<Result<Snapshot,DeviceError>>), Capabilities(mpsc::Sender<Result<Capabilities,DeviceError>>), Apply(Command,mpsc::Sender<Result<ApplyReport,DeviceError>>), RestoreValidationFan(mpsc::Sender<Result<ApplyReport,DeviceError>>), RestoreValidationProfile(mpsc::Sender<Result<ApplyReport,DeviceError>>), ValidateFanCustom70(mpsc::Sender<Result<ApplyReport,DeviceError>>), ValidateLightingBrightness(mpsc::Sender<Result<ApplyReport,DeviceError>>), RestoreValidationLighting(mpsc::Sender<Result<ApplyReport,DeviceError>>), ValidateStaticColors(mpsc::Sender<Result<ApplyReport,DeviceError>>), RestoreStaticColors(mpsc::Sender<Result<ApplyReport,DeviceError>>), ValidateBatteryCare(mpsc::Sender<Result<ApplyReport,DeviceError>>), RestoreBatteryCare(mpsc::Sender<Result<ApplyReport,DeviceError>>), ValidateDynamic(LightingEffect,mpsc::Sender<Result<ApplyReport,DeviceError>>), RestoreDynamic(mpsc::Sender<Result<ApplyReport,DeviceError>>) }
fn stopped() -> DeviceError { DeviceError{kind:ErrorKind::WorkerStopped,message:"Device worker stopped".into()} }
impl thermal_runtime::ThermalIo for platform::Backend {
    fn ready(&mut self)->Result<bool,DeviceError>{Ok(self.capabilities()?.items.iter().any(|c|c.id=="fan_custom"&&c.writable&&c.status==CapabilityStatus::Verified))}
    fn read(&mut self)->Result<Snapshot,DeviceError>{self.thermal_snapshot()}
    fn write(&mut self,cpu:u8,gpu:u8)->Result<ApplyReport,DeviceError>{self.apply(Command::SetFanSpeeds{cpu_percent:cpu,gpu_percent:gpu})}
    fn restore_auto(&mut self)->Result<ApplyReport,DeviceError>{self.restore_thermal_auto()}
}
impl Device {
    pub fn connect() -> Result<Self,DeviceError> { Self::connect_with_policy(WritePolicy::ReadOnly) }
    pub fn connect_with_policy(policy: WritePolicy) -> Result<Self,DeviceError> {
        let (sender,rx)=mpsc::channel(); let (ready_tx,ready_rx)=mpsc::channel();
        std::thread::Builder::new().name("acer-wmi".into()).spawn(move || {
            let mut backend=match platform::Backend::connect(policy) { Ok(b)=>{let _=ready_tx.send(Ok(()));b}, Err(e)=>{let _=ready_tx.send(Err(e));return} };
            let mut thermal: Option<thermal_runtime::Session> = None;
            let mut next_tick=std::time::Instant::now();
            loop {
                if std::time::Instant::now()>=next_tick {
                    if let Some(session)=thermal.as_mut() { session.tick(&mut backend); }
                    next_tick=std::time::Instant::now()+std::time::Duration::from_secs(1);
                }
                let received=if thermal.as_ref().is_some_and(|s|s.active()) {rx.recv_timeout(next_tick.saturating_duration_since(std::time::Instant::now()))} else {rx.recv().map_err(|_|mpsc::RecvTimeoutError::Disconnected)};
                let request=match received {
                    Ok(request)=>request,
                    Err(mpsc::RecvTimeoutError::Timeout)=>continue,
                    Err(mpsc::RecvTimeoutError::Disconnected)=>{if let Some(session)=thermal.as_mut(){session.stop(&mut backend,"Device owner disconnected");}break;}
                };
                match request {
                    Request::StartThermal(config,tx)=>{
                        let result=if thermal.as_ref().is_some_and(|s|s.owns_fans()) {Err(DeviceError{kind:ErrorKind::InvalidArgument,message:"Stop the current curve and confirm restoration before restarting".into()})} else {thermal_runtime::Session::start(config,&mut backend).map(|session|{let status=session.status();thermal=Some(session);status})};
                        let _=tx.send(result);
                    },
                    Request::StopThermal(tx)=>{if let Some(session)=thermal.as_mut(){session.stop(&mut backend,"Stopped by caller");}let _=tx.send(Ok(thermal.as_ref().map(|s|s.status()).unwrap_or_default()));},
                    Request::ThermalStatus(tx)=>{let _=tx.send(Ok(thermal.as_ref().map(|s|s.status()).unwrap_or_default()));}, Request::SaveLightingCheckpoint(tx)=>{let _=tx.send(backend.save_validation_lighting_checkpoint());},Request::Snapshot(tx)=>{let _=tx.send(backend.snapshot());},Request::Capabilities(tx)=>{let _=tx.send(backend.capabilities());},Request::Apply(command,tx)=>{let conflicts=matches!(command,Command::SetFanMode{..}|Command::SetFanSpeeds{..}|Command::SetProfile{..});let result=if conflicts&&thermal.as_ref().is_some_and(|s|s.owns_fans()){Err(DeviceError{kind:ErrorKind::InvalidArgument,message:"Stop thermal control and confirm Auto restoration before changing fan or performance mode".into()})}else{backend.apply(command)};let _=tx.send(result);},Request::RestoreValidationFan(tx)=>{let _=tx.send(backend.restore_validation_fan());},Request::RestoreValidationProfile(tx)=>{let _=tx.send(backend.restore_validation_profile());},Request::ValidateFanCustom70(tx)=>{let _=tx.send(backend.begin_fan_custom_70_validation());},Request::ValidateLightingBrightness(tx)=>{let _=tx.send(backend.begin_lighting_brightness_validation());},Request::RestoreValidationLighting(tx)=>{let _=tx.send(backend.restore_validation_lighting());},Request::ValidateStaticColors(tx)=>{let _=tx.send(backend.begin_static_colors_validation());},Request::RestoreStaticColors(tx)=>{let _=tx.send(backend.restore_validation_static_colors());},Request::ValidateBatteryCare(tx)=>{let _=tx.send(backend.begin_battery_care_validation());},Request::RestoreBatteryCare(tx)=>{let _=tx.send(backend.restore_validation_battery_care());},Request::ValidateDynamic(effect,tx)=>{let _=tx.send(backend.begin_dynamic_lighting_validation(effect));},Request::RestoreDynamic(tx)=>{let _=tx.send(backend.restore_validation_dynamic_lighting());} } }
        }).map_err(|e|DeviceError{kind:ErrorKind::Transport,message:e.to_string()})?;
        ready_rx.recv().map_err(|_|stopped())??; Ok(Self{sender})
    }
    pub fn snapshot(&self)->Result<Snapshot,DeviceError>{let(tx,rx)=mpsc::channel();self.sender.send(Request::Snapshot(tx)).map_err(|_|stopped())?;rx.recv().map_err(|_|stopped())?}
    pub fn start_thermal(&self,config:ThermalConfig)->Result<ThermalStatus,DeviceError>{let(tx,rx)=mpsc::channel();self.sender.send(Request::StartThermal(config,tx)).map_err(|_|stopped())?;rx.recv().map_err(|_|stopped())?}
    pub fn stop_thermal(&self)->Result<ThermalStatus,DeviceError>{let(tx,rx)=mpsc::channel();self.sender.send(Request::StopThermal(tx)).map_err(|_|stopped())?;rx.recv().map_err(|_|stopped())?}
    pub fn thermal_status(&self)->Result<ThermalStatus,DeviceError>{let(tx,rx)=mpsc::channel();self.sender.send(Request::ThermalStatus(tx)).map_err(|_|stopped())?;rx.recv().map_err(|_|stopped())?}
    pub fn capabilities(&self)->Result<Capabilities,DeviceError>{let(tx,rx)=mpsc::channel();self.sender.send(Request::Capabilities(tx)).map_err(|_|stopped())?;rx.recv().map_err(|_|stopped())?}
    pub fn apply(&self,command:Command)->Result<ApplyReport,DeviceError>{let(tx,rx)=mpsc::channel();self.sender.send(Request::Apply(command,tx)).map_err(|_|stopped())?;rx.recv().map_err(|_|stopped())?}
    pub fn restore_previous_lighting(&self)->Result<ApplyReport,DeviceError>{self.apply(Command::RestoreLighting{})}
    /// Native supervised validation only: read and retain an independent static
    /// checkpoint without writing. No caller frame or WebView command exists.
    pub fn save_validation_lighting_checkpoint(&self)->Result<ApplyReport,DeviceError>{let(tx,rx)=mpsc::channel();self.sender.send(Request::SaveLightingCheckpoint(tx)).map_err(|_|stopped())?;rx.recv().map_err(|_|stopped())?}
    /// Native validation only. Restores the Auto/Max value saved inside this worker
    /// immediately before a supervised Max write. Accepts no user supplied value.
    pub fn restore_validation_fan(&self)->Result<ApplyReport,DeviceError>{let(tx,rx)=mpsc::channel();self.sender.send(Request::RestoreValidationFan(tx)).map_err(|_|stopped())?;rx.recv().map_err(|_|stopped())?}
    pub fn restore_validation_profile(&self)->Result<ApplyReport,DeviceError>{let(tx,rx)=mpsc::channel();self.sender.send(Request::RestoreValidationProfile(tx)).map_err(|_|stopped())?;rx.recv().map_err(|_|stopped())?}
    pub fn begin_fan_custom_70_validation(&self)->Result<ApplyReport,DeviceError>{let(tx,rx)=mpsc::channel();self.sender.send(Request::ValidateFanCustom70(tx)).map_err(|_|stopped())?;rx.recv().map_err(|_|stopped())?}
    pub fn begin_lighting_brightness_validation(&self)->Result<ApplyReport,DeviceError>{let(tx,rx)=mpsc::channel();self.sender.send(Request::ValidateLightingBrightness(tx)).map_err(|_|stopped())?;rx.recv().map_err(|_|stopped())?}
    pub fn restore_validation_lighting(&self)->Result<ApplyReport,DeviceError>{let(tx,rx)=mpsc::channel();self.sender.send(Request::RestoreValidationLighting(tx)).map_err(|_|stopped())?;rx.recv().map_err(|_|stopped())?}
    pub fn begin_static_colors_validation(&self)->Result<ApplyReport,DeviceError>{let(tx,rx)=mpsc::channel();self.sender.send(Request::ValidateStaticColors(tx)).map_err(|_|stopped())?;rx.recv().map_err(|_|stopped())?}
    pub fn restore_validation_static_colors(&self)->Result<ApplyReport,DeviceError>{let(tx,rx)=mpsc::channel();self.sender.send(Request::RestoreStaticColors(tx)).map_err(|_|stopped())?;rx.recv().map_err(|_|stopped())?}
    pub fn begin_battery_care_validation(&self)->Result<ApplyReport,DeviceError>{let(tx,rx)=mpsc::channel();self.sender.send(Request::ValidateBatteryCare(tx)).map_err(|_|stopped())?;rx.recv().map_err(|_|stopped())?}
    pub fn restore_validation_battery_care(&self)->Result<ApplyReport,DeviceError>{let(tx,rx)=mpsc::channel();self.sender.send(Request::RestoreBatteryCare(tx)).map_err(|_|stopped())?;rx.recv().map_err(|_|stopped())?}
    pub fn begin_dynamic_lighting_validation(&self,effect:LightingEffect)->Result<ApplyReport,DeviceError>{let(tx,rx)=mpsc::channel();self.sender.send(Request::ValidateDynamic(effect,tx)).map_err(|_|stopped())?;rx.recv().map_err(|_|stopped())?}
    pub fn restore_validation_dynamic_lighting(&self)->Result<ApplyReport,DeviceError>{let(tx,rx)=mpsc::channel();self.sender.send(Request::RestoreDynamic(tx)).map_err(|_|stopped())?;rx.recv().map_err(|_|stopped())?}
}
#[cfg(test)] mod policy_tests{
    use super::*;
    #[test] fn verified_policy_is_bound_to_target_and_exact_commands(){
        for mode in [FanMode::Auto,FanMode::Max]{let command=Command::SetFanMode{mode};assert!(policy_allows(WritePolicy::Verified,"Predator PHN16-71","V1.18",&command));assert!(!policy_allows(WritePolicy::ReadOnly,"Predator PHN16-71","V1.18",&command));assert!(!policy_allows(WritePolicy::Verified,"PHN16-72","V1.18",&command));assert!(!policy_allows(WritePolicy::Verified,"PHN16-71","V1.19",&command));}
        for profile in [Profile::Quiet,Profile::Balanced,Profile::Performance,Profile::Turbo]{assert!(policy_allows(WritePolicy::Verified,"PHN16-71","V1.18",&Command::SetProfile{profile}));}
        for percent in [0,69,70,100]{assert!(policy_allows(WritePolicy::Verified,"PHN16-71","V1.18",&Command::SetFanSpeeds{cpu_percent:percent,gpu_percent:percent}));}
        assert!(policy_allows(WritePolicy::Verified,"PHN16-71","V1.18",&Command::SetLighting{lighting:Lighting{brightness:50,zones:[[0;3];4]}}));assert!(policy_allows(WritePolicy::Verified,"PHN16-71","V1.18",&Command::SetBatteryCare{enabled:true}));
        assert!(policy_allows(WritePolicy::Verified,"PHN16-71","V1.18",&Command::SetLightingAnimation{lighting:LightingAnimation{effect:LightingEffect::Wave,speed:3,brightness:20,direction:1,color:[0;3]}}));assert!(policy_allows(WritePolicy::Verified,"PHN16-71","V1.18",&Command::RestoreLighting{}));assert!(!policy_allows(WritePolicy::ReadOnly,"PHN16-71","V1.18",&Command::RestoreLighting{}));
        for command in [Command::SetFanMode{mode:FanMode::Custom},Command::SetProfile{profile:Profile::Eco},Command::SetFanSpeeds{cpu_percent:101,gpu_percent:70},Command::SetFanSpeeds{cpu_percent:70,gpu_percent:101},Command::SetLighting{lighting:Lighting{brightness:101,zones:[[0;3];4]}}]{assert!(!policy_allows(WritePolicy::Verified,"PHN16-71","V1.18",&command));}
    }
}
#[cfg(windows)] mod platform;
#[cfg(not(windows))] mod platform {
    use super::*;
    pub struct Backend;
    impl Backend {
        pub fn connect(_:WritePolicy)->Result<Self,DeviceError>{Err(DeviceError{kind:ErrorKind::Unsupported,message:"Acer WMI backend requires Windows".into()})}
        pub fn snapshot(&mut self)->Result<Snapshot,DeviceError>{Err(stopped())}
        pub fn thermal_snapshot(&mut self)->Result<Snapshot,DeviceError>{Err(stopped())}
        pub fn capabilities(&mut self)->Result<Capabilities,DeviceError>{Err(stopped())}
        pub fn apply(&mut self,_:Command)->Result<ApplyReport,DeviceError>{Err(stopped())}
        pub fn restore_thermal_auto(&mut self)->Result<ApplyReport,DeviceError>{Err(stopped())}
        pub fn restore_validation_fan(&mut self)->Result<ApplyReport,DeviceError>{Err(stopped())}
        pub fn restore_validation_profile(&mut self)->Result<ApplyReport,DeviceError>{Err(stopped())}
        pub fn begin_fan_custom_70_validation(&mut self)->Result<ApplyReport,DeviceError>{Err(stopped())}
        pub fn begin_lighting_brightness_validation(&mut self)->Result<ApplyReport,DeviceError>{Err(stopped())}
        pub fn restore_validation_lighting(&mut self)->Result<ApplyReport,DeviceError>{Err(stopped())}
        pub fn begin_static_colors_validation(&mut self)->Result<ApplyReport,DeviceError>{Err(stopped())}
        pub fn restore_validation_static_colors(&mut self)->Result<ApplyReport,DeviceError>{Err(stopped())}
        pub fn begin_battery_care_validation(&mut self)->Result<ApplyReport,DeviceError>{Err(stopped())}
        pub fn restore_validation_battery_care(&mut self)->Result<ApplyReport,DeviceError>{Err(stopped())}
        pub fn begin_dynamic_lighting_validation(&mut self,_:LightingEffect)->Result<ApplyReport,DeviceError>{Err(stopped())}
        pub fn save_validation_lighting_checkpoint(&mut self)->Result<ApplyReport,DeviceError>{Err(stopped())}
        pub fn restore_validation_dynamic_lighting(&mut self)->Result<ApplyReport,DeviceError>{Err(stopped())}
    }
}
