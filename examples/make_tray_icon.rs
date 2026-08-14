//! Generate assets/tray/icon.png.
//!
//! Source of truth is the cow-cat head portrait packed by:
//!
//! ```text
//! python tools/pack_tray_icon.py
//! ```
//!
//! This example only rebuilds a crude fallback if icon.png is missing.

use image::imageops::FilterType;

fn main() {
    if std::path::Path::new("assets/tray/icon.png").exists() {
        println!("assets/tray/icon.png already present (cow-cat avatar).");
        println!("To rebuild: python tools/pack_tray_icon.py");
        return;
    }

    let candidates = [
        "assets/pets/cow-cat/idle_blink/000.png",
        "assets/pets/cow-cat/_master/sit_master.png",
    ];
    let mut img = None;
    for c in candidates {
        if std::path::Path::new(c).exists() {
            img = Some(image::open(c).expect("open"));
            println!("fallback using {c}");
            break;
        }
    }
    let img = img.expect("no pet image");
    let rgba = img.to_rgba8();
    let icon = image::imageops::resize(&rgba, 32, 32, FilterType::Lanczos3);
    std::fs::create_dir_all("assets/tray").ok();
    icon.save("assets/tray/icon.png").expect("save");
    println!(
        "wrote assets/tray/icon.png {}x{}",
        icon.width(),
        icon.height()
    );
}
