use image::{Rgba, RgbaImage};
// Inline a copy of the new rasterizer logic for visual QA
use fontdue::{Font, FontSettings};

fn main() {
    // Load same font order as app
    let candidates: &[(&str, u32)] = &[
        (r"C:\Windows\Fonts\msyh.ttc", 0),
        (r"C:\Windows\Fonts\simhei.ttf", 0),
    ];
    let mut font = None;
    for &(path, idx) in candidates {
        let bytes = std::fs::read(path).unwrap();
        let f = Font::from_bytes(bytes.as_slice(), FontSettings { collection_index: idx, scale: 40.0, ..Default::default() }).unwrap();
        let (_, bmp) = f.rasterize('管', 20.0);
        if bmp.iter().filter(|&&v| v > 16).count() > 40 {
            println!("using {path}");
            font = Some(f);
            break;
        }
    }
    let font = font.unwrap();

    let labels = [
        ("快捷启动", 17.0),
        ("打开常用应用", 12.0),
        ("添加应用", 15.0),
        ("管理", 14.0),
        ("暂停提醒", 14.0),
        ("轻点关闭", 11.0),
        ("暂无常用应用", 14.0),
        ("点上方「添加应用」开始", 12.0),
    ];

    // Render each label with NEW layout algorithm
    let mut y = 10u32;
    let mut img = RgbaImage::from_pixel(480, 320, Rgba([255,255,255,255]));
    // draw fake blue button
    for py in 70..114 {
        for px in 170..450 {
            img.put_pixel(px, py, Rgba([0,122,255,255]));
        }
    }
    for (text, px) in labels {
        let (tw, th, rgba) = rasterize(&font, text, 300, px, [0,0,0,255]);
        // center "添加应用" on blue
        let (dx, dy) = if text == "添加应用" {
            (170 + (280 - tw as i32)/2, 70 + (44 - th as i32)/2 + 1)
        } else if text == "快捷启动" {
            (170, 16)
        } else if text == "打开常用应用" {
            (170, 38)
        } else {
            (20i32, y as i32)
        };
        blit(&mut img, &rgba, tw, th, dx.max(0) as u32, dy.max(0) as u32);
        if text != "添加应用" && text != "快捷启动" && text != "打开常用应用" {
            y += th + 8;
        }
        println!("{text}: {tw}x{th}");
    }
    img.save("target/font_probe/menu_text_qa.png").unwrap();
    println!("wrote target/font_probe/menu_text_qa.png");
}

struct G { metrics: fontdue::Metrics, bitmap: Vec<u8> }

fn rasterize(font: &Font, text: &str, max_width: u32, px: f32, color: [u8;4]) -> (u32,u32,Vec<u8>) {
    let mut glyphs = Vec::new();
    let mut cur_w = 0.0f32;
    for ch in text.chars() {
        let (m, b) = font.rasterize(ch, px);
        cur_w += m.advance_width.max(1.0);
        if cur_w > max_width as f32 { break; }
        glyphs.push(G { metrics: m, bitmap: b });
    }
    let mut min_y = 0i32; let mut max_y = 1i32; let mut any=false; let mut w=0.0f32;
    for g in &glyphs {
        w += g.metrics.advance_width.max(1.0);
        if g.metrics.width==0 { continue; }
        let t=g.metrics.ymin; let b=g.metrics.ymin+g.metrics.height as i32;
        if !any { min_y=t; max_y=b; any=true; } else { min_y=min_y.min(t); max_y=max_y.max(b); }
    }
    let pad=2i32;
    let width=(w.ceil() as i32+pad*2).max(4) as u32;
    let height=(max_y-min_y+pad*2).max(4) as u32;
    let mut rgba=vec![0u8;(width*height*4) as usize];
    let baseline = pad - min_y;
    let mut x = pad as f32;
    for g in &glyphs {
        if g.metrics.width==0 { x+=g.metrics.advance_width.max(1.0); continue; }
        let gx = x as i32 + g.metrics.xmin;
        let gy = baseline + g.metrics.ymin;
        for row in 0..g.metrics.height as i32 {
            for col in 0..g.metrics.width as i32 {
                let px_x=gx+col; let px_y=gy+row;
                if px_x<0||px_y<0||px_x>=width as i32||px_y>=height as i32 {continue;}
                let cov=g.bitmap[(row as usize)*g.metrics.width+col as usize] as f32/255.0;
                if cov<0.04 {continue;}
                let i=((px_y as u32*width+px_x as u32)*4) as usize;
                let a=(color[3] as f32*cov) as u8;
                if a>rgba[i+3]{ rgba[i]=color[0];rgba[i+1]=color[1];rgba[i+2]=color[2];rgba[i+3]=a; }
            }
        }
        x+=g.metrics.advance_width.max(1.0);
    }
    // crop
    let mut x0=width as i32; let mut y0=height as i32; let mut x1=-1; let mut y1=-1;
    for y in 0..height as i32 { for x in 0..width as i32 {
        let i=((y as u32*width+x as u32)*4) as usize;
        if rgba[i+3]>8 { x0=x0.min(x); y0=y0.min(y); x1=x1.max(x); y1=y1.max(y); }
    }}
    if x1<x0 { return (1,1,vec![0,0,0,0]); }
    x0=(x0-1).max(0); y0=(y0-1).max(0); x1=(x1+1).min(width as i32-1); y1=(y1+1).min(height as i32-1);
    let nw=(x1-x0+1) as u32; let nh=(y1-y0+1) as u32;
    let mut out=vec![0u8;(nw*nh*4) as usize];
    for y in 0..nh { for x in 0..nw {
        let si=(((y0 as u32+y)*width+(x0 as u32+x))*4) as usize;
        let di=((y*nw+x)*4) as usize;
        out[di..di+4].copy_from_slice(&rgba[si..si+4]);
    }}
    (nw,nh,out)
}

fn blit(img: &mut RgbaImage, src: &[u8], sw: u32, sh: u32, dx: u32, dy: u32) {
    for y in 0..sh {
        for x in 0..sw {
            let i=((y*sw+x)*4) as usize;
            let a=src[i+3] as f32/255.0;
            if a<0.04 {continue;}
            let tx=dx+x; let ty=dy+y;
            if tx>=img.width()||ty>=img.height(){continue;}
            let p=img.get_pixel_mut(tx,ty);
            for k in 0..3 {
                p.0[k]=((src[i+k] as f32)*a + p.0[k] as f32*(1.0-a)) as u8;
            }
        }
    }
}
