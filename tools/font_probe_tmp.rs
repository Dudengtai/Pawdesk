fn main() {
    let candidates: &[(&str, u32)] = &[
        (r"C:\Windows\Fonts\simhei.ttf", 0),
        (r"C:\Windows\Fonts\msyh.ttc", 0),
        (r"C:\Windows\Fonts\msyh.ttc", 1),
        (r"C:\Windows\Fonts\msyhbd.ttc", 0),
        (r"C:\Windows\Fonts\simsun.ttc", 0),
        (r"C:\Windows\Fonts\msyh.ttc", 2),
    ];
    let texts = ["快捷启动", "添加应用", "管理", "暂停提醒", "轻点关闭", "暂无常用应用"];
    for &(path, idx) in candidates {
        if !std::path::Path::new(path).exists() {
            println!("MISS {path}");
            continue;
        }
        let bytes = std::fs::read(path).unwrap();
        let settings = fontdue::FontSettings { collection_index: idx, scale: 40.0, ..Default::default() };
        match fontdue::Font::from_bytes(bytes.as_slice(), settings) {
            Ok(font) => {
                let (m, bmp) = font.rasterize('管', 24.0);
                let ink = bmp.iter().filter(|&&v| v > 16).count();
                let lm = font.horizontal_line_metrics(24.0);
                println!("OK {path} idx={idx} ink={ink} w={} h={} xmin={} ymin={} adv={:.1} lm={:?}",
                    m.width, m.height, m.xmin, m.ymin, m.advance_width, lm.map(|l| (l.ascent, l.descent, l.line_gap)));
                // write sample strip
                let px = 22.0f32;
                let mut xoff = 8i32;
                let mut canvas = vec![255u8; 600*40*4];
                for t in texts {
                    for ch in t.chars() {
                        let (metrics, bitmap) = font.rasterize(ch, px);
                        let baseline = 28i32;
                        let gx = xoff + metrics.xmin;
                        let gy = baseline + metrics.ymin;
                        for row in 0..metrics.height as i32 {
                            for col in 0..metrics.width as i32 {
                                let px_x = gx + col;
                                let px_y = gy + row;
                                if px_x < 0 || px_y < 0 || px_x >= 600 || px_y >= 40 { continue; }
                                let cov = bitmap[(row as usize)*(metrics.width)+col as usize] as f32 / 255.0;
                                if cov < 0.05 { continue; }
                                let i = ((px_y as u32 * 600 + px_x as u32) * 4) as usize;
                                canvas[i] = 0; canvas[i+1]=0; canvas[i+2]=0;
                                canvas[i+3] = (255.0*cov) as u8;
                            }
                        }
                        xoff += metrics.advance_width.max(1.0) as i32;
                    }
                    xoff += 12;
                }
                let name = format!("font_test_{}_{}.png", path.replace('\\','_').replace(':','').replace('.','_'), idx);
                // write simple RGBA via image crate if available - else raw
                let _ = std::fs::write(format!("{}.raw", name), &canvas);
                // try png with minimal writer
                write_png(&name, 600, 40, &canvas);
                println!("  wrote {name}");
            }
            Err(e) => println!("ERR {path} idx={idx}: {e}"),
        }
    }
}

fn write_png(path: &str, w: u32, h: u32, rgba: &[u8]) {
    // minimal uncompressed-ish via png crate? use stb style raw dump if no png
    // Use image from project deps
}
