//! Read-only native calibration observation; no control call.
fn main()->Result<(),acer_device::DeviceError>{
    #[cfg(windows)]{
        let hardware=acer_device::calibration_hardware::Hardware::connect(acer_device::WritePolicy::ReadOnly)?;
        println!("{:#?}",hardware.observe());
    }
    Ok(())
}
