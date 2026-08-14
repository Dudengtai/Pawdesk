//! Generate assets/tray/icon.png.
//!
//! Preferred source is `assets/tray/cat.svg` (line-art tray mark).
//! Prefer the Node helper (needs @resvg/resvg-js once):
//!
//! ```text
//! npm.cmd install --no-save @resvg/resvg-js
//! node tools/svg_to_tray_icon.js
//! ```
//!
//! Fallback: rasterize a pet sprite frame to 32×32 when SVG tools are unavailable.

use image::imageops::FilterType;

fn main() {
    if std::path::Path::new("assets/tray/icon.png").exists()
        && std::path::Path::new("assets/tray/cat.svg").exists()
    {
        println!("assets/tray/icon.png already present (from cat.svg pipeline).");
        println!("To rebuild from assets/tray/cat.svg: node tools/svg_to_tray_icon.js");
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
