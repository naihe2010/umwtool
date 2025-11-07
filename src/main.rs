use std::error::Error;
use std::thread;
use std::time::Duration;

use tray_item::{IconSource, TrayItem};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::*;
use x11rb::rust_connection::RustConnection;

const KNOWN_WIDTHS: [u32; 5] = [640, 476, 560, 320, 232];

fn main() -> Result<(), Box<dyn Error>> {
    gtk::init()?;
    println!("Starting WXWork shadow-window killer …");

    let mut tray = TrayItem::new("WXWork Killer", IconSource::Resource("umwtool"))?;
    tray.add_menu_item("Quit", || std::process::exit(0))?;

    thread::spawn(move || loop {
        if let Err(e) = kill_shadow() {
            eprintln!("kill_shadow error: {}", e);
        }
        thread::sleep(Duration::from_secs(1));
    });

    gtk::main();
    Ok(())
}

fn kill_shadow() -> Result<(), Box<dyn Error>> {
    let (conn, screen_num) = RustConnection::connect(None)?;
    let screen = &conn.setup().roots[screen_num];

    let tree = conn.query_tree(screen.root)?.reply()?;
    for &win in &tree.children {
        let attrs = conn.get_window_attributes(win)?.reply()?;
        if attrs.map_state != MapState::VIEWABLE {
            continue;
        }

        let (cls, _wm_name) = window_class_and_name(&conn, win)?;
        if !cls.contains("wxwork.exe") {
            continue;
        }

        let geo = conn.get_geometry(win)?.reply()?;
        let (w, h) = (geo.width as u32, geo.height as u32);

        if KNOWN_WIDTHS.contains(&w) {
            continue;
        }
        if h > 20 && h < 100 && w as f64 / h as f64 > 30.0 {
            println!("unmapping shadow window {:x} ({}x{})", win, w, h);
            conn.unmap_window(win)?;
            conn.flush()?;
        }
    }
    Ok(())
}

fn window_class_and_name(
    conn: &RustConnection,
    win: Window,
) -> Result<(String, String), Box<dyn Error>> {
    let cls_atom = conn.intern_atom(false, b"WM_CLASS")?.reply()?.atom;
    let name_atom = conn.intern_atom(false, b"WM_NAME")?.reply()?.atom;

    let cls = get_text_property(conn, win, cls_atom)?;
    let name = get_text_property(conn, win, name_atom)?;
    Ok((cls, name))
}

fn get_text_property(
    conn: &RustConnection,
    win: Window,
    prop: Atom,
) -> Result<String, Box<dyn Error>> {
    let reply = conn
        .get_property(false, win, prop, AtomEnum::STRING, 0, 1024)?
        .reply()?;
    Ok(String::from_utf8_lossy(&reply.value).trim().to_string())
}
