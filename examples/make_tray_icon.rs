use image::imageops::FilterType;
use image::GenericImageView;
fn main() {
    let candidates = [
        "assets/pets/cow-cat/idle_blink/00.png",
        "assets/pets/cow-cat/_master/base_sit.png",
        "assets/pets/cow-cat/_master/base_sit_128.png",
    ];
    let mut img = None;
    for c in candidates {
        if std::path::Path::new(c).exists() {
            img = Some(image::open(c).expect("open"));
            println!("using {c}");
            break;
        }
    }
    let img = img.expect("no pet image");
    let rgba = img.to_rgba8();
    // Crop to opaque bounds roughly by scaling whole image to 32x32
    let icon = image::imageops::resize(&rgba, 32, 32, FilterType::Lanczos3);
    std::fs::create_dir_all("assets/tray").ok();
    icon.save("assets/tray/icon.png").expect("save");
    println!("wrote assets/tray/icon.png {}x{}", icon.width(), icon.height());
}
