use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use gtk::prelude::*;
use tray_item::{IconSource, TrayItem};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::*;
use x11rb::rust_connection::RustConnection;

const KNOWN_WIDTHS: [u32; 5] = [640, 476, 560, 320, 232];

fn main() -> Result<(), Box<dyn Error>> {
    gtk::init()?;
    println!("Starting umwtool …");

    let targets = Arc::new(Mutex::new(load_targets()?));

    let (ui_tx, ui_rx) = glib::MainContext::channel(glib::Priority::default());
    {
        let targets = Arc::clone(&targets);
        ui_rx.attach(None, move |_| {
            show_manager(Arc::clone(&targets));
            glib::ControlFlow::Continue
        });
    }

    let mut tray = TrayItem::new("umwtool", IconSource::Resource("umwtool"))?;
    {
        let ui_tx = ui_tx.clone();
        tray.add_menu_item("Manage unmap list", move || {
            let _ = ui_tx.send(());
        })?;
    }
    tray.add_menu_item("Quit", || std::process::exit(0))?;

    thread::spawn(move || loop {
        let snapshot = { targets.lock().unwrap().clone() };
        if let Err(e) = kill_shadow(&snapshot) {
            eprintln!("kill_shadow error: {}", e);
        }
        thread::sleep(Duration::from_secs(1));
    });

    gtk::main();
    Ok(())
}

fn kill_shadow(targets: &[String]) -> Result<(), Box<dyn Error>> {
    let (conn, screen_num) = RustConnection::connect(None)?;
    let screen = &conn.setup().roots[screen_num];

    let tree = conn.query_tree(screen.root)?.reply()?;
    for &win in &tree.children {
        let attrs = conn.get_window_attributes(win)?.reply()?;
        if attrs.map_state != MapState::VIEWABLE {
            continue;
        }

        let (cls, _wm_name) = window_class_and_name(&conn, win)?;
        if !matches_any_target(&cls, targets) {
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

fn matches_any_target(wm_class: &str, targets: &[String]) -> bool {
    let cls = wm_class.to_ascii_lowercase();
    targets
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .any(|t| cls.contains(&t.to_ascii_lowercase()))
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

fn config_file() -> Result<PathBuf, Box<dyn Error>> {
    let base = if let Ok(p) = std::env::var("XDG_CONFIG_HOME") {
        PathBuf::from(p)
    } else {
        let home = std::env::var("HOME")?;
        PathBuf::from(home).join(".config")
    };
    Ok(base.join("umwtool").join("targets.txt"))
}

fn default_targets() -> Vec<String> {
    vec!["wxwork.exe".to_string()]
}

fn load_targets() -> Result<Vec<String>, Box<dyn Error>> {
    let path = config_file()?;
    if let Ok(s) = fs::read_to_string(&path) {
        let mut v: Vec<String> = s
            .lines()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty())
            .map(|l| l.to_string())
            .collect();
        if v.is_empty() {
            v = default_targets();
            save_targets(&v)?;
        }
        return Ok(v);
    }
    let v = default_targets();
    save_targets(&v)?;
    Ok(v)
}

fn save_targets(targets: &[String]) -> Result<(), Box<dyn Error>> {
    let path = config_file()?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut s = String::new();
    for t in targets.iter().map(|s| s.trim()).filter(|s| !s.is_empty()) {
        s.push_str(t);
        s.push('\n');
    }
    fs::write(path, s)?;
    Ok(())
}

fn show_manager(targets: Arc<Mutex<Vec<String>>>) {
    let win = gtk::Window::new(gtk::WindowType::Toplevel);
    win.set_title("unmap list");
    win.set_default_size(420, 320);

    let root = gtk::Box::new(gtk::Orientation::Vertical, 8);
    root.set_margin_top(12);
    root.set_margin_bottom(12);
    root.set_margin_start(12);
    root.set_margin_end(12);

    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::Single);

    let row_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let entry = gtk::Entry::new();
    entry.set_hexpand(true);
    let add_btn = gtk::Button::with_label("Add");
    let del_btn = gtk::Button::with_label("Remove");
    row_box.pack_start(&entry, true, true, 0);
    row_box.pack_start(&add_btn, false, false, 0);
    row_box.pack_start(&del_btn, false, false, 0);

    root.pack_start(&list, true, true, 0);
    root.pack_start(&row_box, false, false, 0);

    win.add(&root);

    {
	let prog_names = targets.lock().unwrap();
	refresh_list(&list, &prog_names);
    }

    {
        let targets = Arc::clone(&targets);
        let list = list.clone();
        let entry = entry.clone();
        add_btn.connect_clicked(move |_| {
            let s = entry.text().trim().to_string();
            if s.is_empty() {
                return;
            }
            entry.set_text("");
            let mut v = targets.lock().unwrap();
            if !v.iter().any(|x| x.eq_ignore_ascii_case(&s)) {
                v.push(s);
                let _ = save_targets(&v);
                refresh_list(&list, &v);
            }
        });
    }

    {
        let targets = Arc::clone(&targets);
        let list = list.clone();
        del_btn.connect_clicked(move |_| {
            if let Some(row) = list.selected_row() {
                if let Some(child) = row.child() {
                    if let Ok(label) = child.downcast::<gtk::Label>() {
                        let text = label.text().to_string();
                        let mut v = targets.lock().unwrap();
                        v.retain(|x| !x.eq_ignore_ascii_case(&text));
                        let _ = save_targets(&v);
                        refresh_list(&list, &v);
                    }
                }
            }
        });
    }

    win.show_all();
}

fn refresh_list(list: &gtk::ListBox, targets: &[String]) {
    for child in list.children() {
        list.remove(&child);
    }
    let mut v = Vec::from_iter(targets);
    v.sort_by_key(|a| a.to_ascii_lowercase());
    for t in v {
        let label = gtk::Label::new(Some(t));
        label.set_xalign(0.0);
        list.add(&label);
    }
    list.show_all();
}
