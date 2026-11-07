use acer_broker::VendorSetting;
fn main(){
    let args=std::env::args().skip(1).collect::<Vec<_>>();
    if args.is_empty(){println!("{}",acer_broker::query_vendor_controls());return}
    let setting=match args.as_slice(){
        [flag,name] if flag=="--validate-restore"=>match name.as_str(){"lcd_overdrive"=>Some(VendorSetting::LcdOverdrive),"boot_sound"=>Some(VendorSetting::BootSound),"sticky_key"=>Some(VendorSetting::StickyKey),"backlight_timeout"=>Some(VendorSetting::BacklightTimeout),_=>None},_=>None,
    };
    let Some(setting)=setting else{eprintln!("No arguments: read only. Explicit: --validate-restore lcd_overdrive|boot_sound|sticky_key|backlight_timeout");std::process::exit(2)};
    match acer_broker::validate_vendor_control(setting){Ok(report)=>{println!("{report}");if report["success"]!=true{std::process::exit(1)}},Err(error)=>{println!("{}",serde_json::json!({"success":false,"error":error}));std::process::exit(1)}}
}
