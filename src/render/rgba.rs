//! Tightly-packed RGBA8 sampling helpers.

/// Bilinear sample of tightly-packed RGBA8. Transparent outside bounds.
pub fn sample_rgba_bilinear(src: &[u8], w: u32, h: u32, x: f64, y: f64) -> [u8; 4] {
    if w == 0 || h == 0 {
        return [0, 0, 0, 0];
    }
    let x0 = x.floor() as i32;
    let y0 = y.floor() as i32;
    let x1 = x0 + 1;
    let y1 = y0 + 1;
    let fx = (x - x0 as f64).clamp(0.0, 1.0);
    let fy = (y - y0 as f64).clamp(0.0, 1.0);

    let p = |ix: i32, iy: i32| -> [f64; 4] {
        if ix < 0 || iy < 0 || ix >= w as i32 || iy >= h as i32 {
            return [0.0, 0.0, 0.0, 0.0];
        }
        let i = ((iy as u32 * w + ix as u32) * 4) as usize;
        [
            src[i] as f64,
            src[i + 1] as f64,
            src[i + 2] as f64,
            src[i + 3] as f64,
        ]
    };

    // Premultiply for correct alpha blend of edge pixels.
    let fetch = |ix: i32, iy: i32| -> [f64; 4] {
        let c = p(ix, iy);
        let a = c[3] / 255.0;
        [c[0] * a, c[1] * a, c[2] * a, c[3]]
    };

    let c00 = fetch(x0, y0);
    let c10 = fetch(x1, y0);
    let c01 = fetch(x0, y1);
    let c11 = fetch(x1, y1);

    let mut out = [0.0f64; 4];
    for i in 0..4 {
        let top = c00[i] * (1.0 - fx) + c10[i] * fx;
        let bot = c01[i] * (1.0 - fx) + c11[i] * fx;
        out[i] = top * (1.0 - fy) + bot * fy;
    }

    let a = out[3].clamp(0.0, 255.0);
    if a < 0.5 {
        return [0, 0, 0, 0];
    }
    let inv = 255.0 / a;
    [
        (out[0] * inv).clamp(0.0, 255.0).round() as u8,
        (out[1] * inv).clamp(0.0, 255.0).round() as u8,
        (out[2] * inv).clamp(0.0, 255.0).round() as u8,
        a.round() as u8,
    ]
}
