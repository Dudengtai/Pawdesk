use fontdue::{Font, FontSettings};
use image::{Rgba, RgbaImage};
use std::path::Path;

fn main() {
    let candidates: &[(&str, u32)] = &[
        (r"C:\Windows\Fonts\simhei.ttf", 0),
        (r"C:\Windows\Fonts\msyh.ttc", 0),
        (r"C:\Windows\Fonts\msyh.ttc", 1),
        (r"C:\Windows\Fonts\msyhbd.ttc", 0),
        (r"C:\Windows\Fonts\simsun.ttc", 0),
        (r"C:\Windows\Fonts\msyh.ttc", 2),
        (r"C:\Windows\Fonts\msjhl.ttc", 0),
        (r"C:\Windows\Fonts\Deng.ttf", 0),
        (r"C:\Windows\Fonts\Dengb.ttf", 0),
        (r"C:\Windows\Fonts\simkai.ttf", 0),
        (r"C:\Windows\Fonts\msyh.ttc", 3),
    ];
    let texts = ["给你叼来了", "想开哪个？", "再叼一个", "喂给我删除", "拍拍收起", "还没叼来应用", "提醒与外观"];
    std::fs::create_dir_all("target/font_probe").ok();
    for &(path, idx) in candidates {
        if !Path::new(path).exists() {
            println!("MISS {path}");
            continue;
        }
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => { println!("READERR {path}: {e}"); continue; }
        };
        let settings = FontSettings { collection_index: idx, scale: 40.0, ..FontSettings::default() };
        let font = match Font::from_bytes(bytes.as_slice(), settings) {
            Ok(f) => f,
            Err(e) => { println!("ERR {path} idx={idx}: {e}"); continue; }
        };
        let (m, bmp) = font.rasterize('管', 24.0);
        let ink = bmp.iter().filter(|&&v| v > 16).count();
        let lm = font.horizontal_line_metrics(24.0);
        println!(
            "OK {path} idx={idx} ink={ink} glyph={}x{} xmin={} ymin={} adv={:.1} lm={:?}",
            m.width, m.height, m.xmin, m.ymin, m.advance_width,
            lm.map(|l| (l.ascent, l.descent, l.line_gap))
        );
        if ink < 20 {
            println!("  skip low ink");
            continue;
        }
        let px = 28.0f32;
        let mut img = RgbaImage::from_pixel(900, 80, Rgba([255, 255, 255, 255]));
        let mut xoff = 10i32;
        let baseline = 55i32;
        for t in &texts {
            for ch in t.chars() {
                let (metrics, bitmap) = font.rasterize(ch, px);
                let gx = xoff + metrics.xmin;
                let gy = baseline + metrics.ymin;
                for row in 0..metrics.height as i32 {
                    for col in 0..metrics.width as i32 {
                        let px_x = gx + col;
                        let px_y = gy + row;
                        if px_x < 0 || px_y < 0 || px_x >= 900 || px_y >= 80 { continue; }
                        let cov = bitmap[(row as usize) * metrics.width + col as usize] as f32 / 255.0;
                        if cov < 0.04 { continue; }
                        let p = img.get_pixel_mut(px_x as u32, px_y as u32);
                        let a = cov;
                        p.0[0] = (p.0[0] as f32 * (1.0 - a)) as u8;
                        p.0[1] = (p.0[1] as f32 * (1.0 - a)) as u8;
                        p.0[2] = (p.0[2] as f32 * (1.0 - a)) as u8;
                        p.0[3] = 255;
                    }
                }
                xoff += metrics.advance_width.max(1.0) as i32;
            }
            xoff += 16;
        }
        let name = format!(
            "target/font_probe/{}_{}.png",
            Path::new(path).file_name().unwrap().to_string_lossy().replace('.', "_"),
            idx
        );
        img.save(&name).unwrap();
        println!("  wrote {name}");
    }
}
