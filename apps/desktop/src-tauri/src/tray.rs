use std::sync::{Arc,atomic::{AtomicBool,Ordering}};
use tauri::{Emitter,Manager, menu::{Menu, MenuItem,Submenu,PredefinedMenuItem}, tray::TrayIconBuilder};

fn icon()->tauri::image::Image<'static> {
    // Small native fan mark. Supersampling keeps the tray and title-bar edges clean.
    let mut rgba=vec![0u8;32*32*4];
    for y in 0..32 {for x in 0..32 {
        let mut sum=[0u32;4];
        for sy in 0..4 {for sx in 0..4 {
            let dx=x as f64+(sx as f64+0.5)/4.0-16.0;
            let dy=y as f64+(sy as f64+0.5)/4.0-16.0;
            if dx*dx+dy*dy>14.0*14.0 {continue}
            let blade=(0..3).any(|i|{let a=i as f64*std::f64::consts::TAU/3.0;let u=dx*a.cos()+dy*a.sin()-5.0;let v=-dx*a.sin()+dy*a.cos()-1.0;u*u/13.0+v*v/30.0<=1.0});
            let color=if dx*dx+dy*dy<=6.25||blade{[255,255,255,255]}else{[38,113,220,255]};
            for c in 0..4{sum[c]+=color[c];}
        }}
        let p=(y*32+x)*4;let coverage=sum[3];
        if coverage>0{for c in 0..3{rgba[p+c]=(sum[c]*255/coverage) as u8;}rgba[p+3]=(coverage/16) as u8;}
    }}
    tauri::image::Image::new_owned(rgba,32,32)
}

pub fn open(app: &tauri::AppHandle) {
    if let Some(window)=app.get_webview_window("main") {
        let _=window.unminimize(); let _=window.show(); let _=window.set_focus();
    } else {
        match tauri::WebviewWindowBuilder::new(app,"main",tauri::WebviewUrl::App("index.html".into()))
            .title("Acer Console").inner_size(1000.0,720.0).min_inner_size(760.0,560.0)
            .decorations(false).shadow(true).transparent(false).build() {
            Ok(window)=>{let _=window.set_icon(icon());},
            Err(error)=>eprintln!("无法打开控制台: {error}"),
        }
    }
}
pub fn setup(app:&tauri::App)->tauri::Result<()> {
    let show=MenuItem::with_id(app,"show","打开控制台",true,None::<&str>)?;
    let quit=MenuItem::with_id(app,"quit","退出",true,None::<&str>)?;
    let auto=MenuItem::with_id(app,"fan_auto","自动散热",true,None::<&str>)?;
    let max=MenuItem::with_id(app,"fan_max","最大转速",true,None::<&str>)?;
    let fans=Submenu::with_items(app,"风扇",true,&[&auto,&max])?;
    let quiet=MenuItem::with_id(app,"profile_quiet","安静",true,None::<&str>)?;
    let balanced=MenuItem::with_id(app,"profile_balanced","均衡",true,None::<&str>)?;
    let performance=MenuItem::with_id(app,"profile_performance","性能",true,None::<&str>)?;
    let turbo=MenuItem::with_id(app,"profile_turbo","极速",true,None::<&str>)?;
    let profiles=Submenu::with_items(app,"性能模式",true,&[&quiet,&balanced,&performance,&turbo])?;
    let status=MenuItem::with_id(app,"status","快捷控制需要已授权的设备连接",false,None::<&str>)?;
    let divider=PredefinedMenuItem::separator(app)?;
    let menu=Menu::with_items(app,&[&show,&fans,&profiles,&status,&divider,&quit])?;
    let busy=Arc::new(AtomicBool::new(false));
    if let Some(window)=app.get_webview_window("main"){window.set_icon(icon())?;}
    TrayIconBuilder::new().icon(icon()).tooltip("Acer Console")
      .menu(&menu).show_menu_on_left_click(true)
      .on_menu_event(move|app,event|{
        use acer_protocol::{Command,FanMode,Profile};
        let command=match event.id.as_ref(){
            "show"=>{open(app);return},"quit"=>{app.exit(0);return},
            "fan_auto"=>Command::SetFanMode{mode:FanMode::Auto},
            "fan_max"=>Command::SetFanMode{mode:FanMode::Max},
            "profile_quiet"=>Command::SetProfile{profile:Profile::Quiet},
            "profile_balanced"=>Command::SetProfile{profile:Profile::Balanced},
            "profile_performance"=>Command::SetProfile{profile:Profile::Performance},
            "profile_turbo"=>Command::SetProfile{profile:Profile::Turbo},
            _=>return,
        };
        if busy.swap(true,Ordering::SeqCst){return}
        let busy=busy.clone();let status=status.clone();let app=app.clone();
        let _=status.set_text("正在应用，请稍候…");
        tauri::async_runtime::spawn(async move{
            let result=async {
                let broker=app.state::<super::BrokerState>().0.clone();
                let authorized=broker.lock().map_err(|e|e.to_string())?.is_some();
                if !authorized{return Err("请打开控制台并授权设备连接".to_string())}
                let local=super::device(&app.state::<super::Backend>())?;
                super::apply(app.clone(),local,broker,command).await
            }.await;
            let message=match result {Ok(value)=>value["message"].as_str().unwrap_or("请打开控制台检查结果").to_owned(),Err(error)=>error};
            let _=status.set_text(&message);
            let _=app.emit("device-refresh",());
            busy.store(false,Ordering::SeqCst);
        });
      }).build(app)?;
    Ok(())
}
