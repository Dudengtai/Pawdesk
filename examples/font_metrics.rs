// Probe: what does current rasterize_text produce?
use fontdue::{Font, FontSettings};

fn main() {
    let bytes = std::fs::read(r"C:\Windows\Fonts\simhei.ttf").unwrap();
    let font = Font::from_bytes(bytes.as_slice(), FontSettings::default()).unwrap();
    let px = 17.0f32;
    let text = "快捷启动";
    
    // Current buggy layout params
    let line_ascent = (px * 0.92).ceil() as i32;
    let line_descent = (px * 0.20).ceil() as i32;
    let line_height = (line_ascent + line_descent).max(px.ceil() as i32 + 2);
    let pad = 2i32;
    let height = line_height + pad * 2;
    println!("buf h={height} ascent={line_ascent} descent={line_descent} line_h={line_height}");
    
    let baseline = pad + line_ascent;
    for ch in text.chars() {
        let (m, bmp) = font.rasterize(ch, px);
        let gy = baseline + m.ymin;
        let bottom = gy + m.height as i32;
        let clipped = bottom > height || gy < 0;
        println!("'{ch}' w={} h={} xmin={} ymin={} adv={:.1} gy={gy} bottom={bottom} clipped={clipped} ink={}",
            m.width, m.height, m.xmin, m.ymin, m.advance_width, bmp.iter().filter(|&&v| v>16).count());
    }

    // Correct approach using actual glyph bounds
    let mut min_y = i32::MAX;
    let mut max_y = i32::MIN;
    let mut x = 0.0f32;
    for ch in text.chars() {
        let (m, _) = font.rasterize(ch, px);
        // place at baseline 0
        min_y = min_y.min(m.ymin);
        max_y = max_y.max(m.ymin + m.height as i32);
        x += m.advance_width;
    }
    println!("tight at baseline0: y=[{min_y},{max_y}] span={} advance={x:.1}", max_y - min_y);

    if let Some(lm) = font.horizontal_line_metrics(px) {
        println!("line metrics ascent={} descent={} gap={}", lm.ascent, lm.descent, lm.line_gap);
    }
}
