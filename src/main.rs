use std::process::Command;
use std::thread;
use std::time::Duration;
use std::error::Error;
use tray_item::{IconSource, TrayItem};
use gtk;

const KNOWN_WIDTHS: [&str; 5] = ["640", "476", "560", "320", "232"];

fn kill_shadow() -> Result<(), Box<dyn Error>> {
    
    let process_output = Command::new("sh")
        .arg("-c")
        .arg("ps -ef | grep WXWork.exe")
        .output()?;

    let process_list = String::from_utf8_lossy(&process_output.stdout);
    
    if process_list.lines().count() <= 1 {
        return Ok(());
    }

    if let Err(e) = check_windows_with_wmctrl() {
        eprintln!("Error checking wmctrl: {}", e);
    }

    if let Err(e) = check_windows_with_xwininfo() {
        eprintln!("Error checking xwininfo: {}", e);
    }

    Ok(())
}

fn check_windows_with_wmctrl() -> Result<(), Box<dyn Error>> {
    let output = Command::new("wmctrl")
        .args(["-l", "-G", "-p", "-x"])
        .output()?;
    
    let output_str = String::from_utf8_lossy(&output.stdout);

    for line in output_str.lines() {
        if !line.contains("wxwork.exe.wxwork.exe") {
            continue;
        }

        let parts: Vec<&str> = line.split_whitespace().collect();

        if let (Some(window_id), Some(width_str)) = (parts.get(0), parts.get(5)) {
            
            if KNOWN_WIDTHS.contains(width_str) {
                continue;
            }

            if parts.len() == 9 {
                unmap_window(window_id)?;
            }
        }
    }
    Ok(())
}

fn check_windows_with_xwininfo() -> Result<(), Box<dyn Error>> {
    let output = Command::new("sh")
        .arg("-c")
        .arg(r#"xwininfo -root -tree | grep wxwork.exe | grep "has no name""#)
        .output()?;

    let output_str = String::from_utf8_lossy(&output.stdout);

    for line in output_str.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();

        if let (Some(window_id), Some(geo_str)) = (parts.get(0), parts.get(6)) {
            
            if let Some(dim_str) = geo_str.split('+').next() {
                let dims: Vec<&str> = dim_str.split('x').collect();
                
                if dims.len() == 2 {
                    if let (Ok(width), Ok(height)) = (dims[0].parse::<f64>(), dims[1].parse::<f64>()) {
                        
                        if height < 100.0 && height > 20.0 && (width / height) > 30.0 {
                            unmap_window(window_id)?;
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

fn unmap_window(window_id: &str) -> Result<(), Box<dyn Error>> {
    let xwininfo_output = Command::new("xwininfo")
        .args(["-id", window_id])
        .output()?;

    let output_str = String::from_utf8_lossy(&xwininfo_output.stdout);

    if output_str.contains("IsUnMapped") {
        return Ok(());
    }

    println!("Unmapping window: {}", window_id);
    Command::new("xdotool")
        .args(["windowunmap", window_id])
        .status()?;
    Ok(())
}


fn main() {
    gtk::init().unwrap();
    println!("Starting WXWork shadow window killer...");

    let mut tray = TrayItem::new("WXWork Killer", IconSource::Resource("icon")).unwrap();

    tray.add_menu_item("Quit", || {
        println!("Quitting WXWork shadow window killer.");
        std::process::exit(0);
    }).unwrap();

    thread::spawn(|| {
        loop {
            if let Err(e) = kill_shadow() {
                eprintln!("An error occurred during shadow kill: {}", e);
            }
            thread::sleep(Duration::from_secs(1));
        }
    });

    gtk::main()
}
