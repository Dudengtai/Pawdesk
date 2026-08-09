//! Probe: how does SHGetFileInfoW render .lnk icons (link overlay arrow)?
//! Saves extracted icons as PNGs under target/icon_probe for visual QA.

#[path = "../src/shortcut/icon.rs"]
mod icon;

use image::{ImageBuffer, Rgba};
use std::path::{Path, PathBuf};

fn find_lnks() -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut dirs = vec![
        std::env::var("USERPROFILE")
            .map(|h| PathBuf::from(h).join("Desktop"))
            .unwrap_or_default(),
        PathBuf::from(r"C:\ProgramData\Microsoft\Windows\Start Menu\Programs"),
        PathBuf::from(
            std::env::var("APPDATA")
                .map(|h| PathBuf::from(h).join(r"Microsoft\Windows\Start Menu\Programs"))
                .unwrap_or_default(),
        ),
    ];
    let mut stack = Vec::new();
    while let Some(d) = dirs.pop() {
        stack.push(d);
    }
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p
                .extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("lnk"))
            {
                out.push(p);
                if out.len() >= 5 {
                    return out;
                }
            }
        }
    }
    out
}

/// Count pixels that differ between two RGBA buffers (bottom-left quadrant).
fn diff_bottom_left(a: &icon::IconRgba, b: &icon::IconRgba) -> usize {
    if a.w != b.w || a.h != b.h {
        return usize::MAX;
    }
    let mut diff = 0;
    for y in (a.h / 2)..a.h {
        for x in 0..(a.w / 2) {
            let i = ((y * a.w + x) * 4) as usize;
            if a.rgba[i..i + 4] != b.rgba[i..i + 4] {
                diff += 1;
            }
        }
    }
    diff
}

fn save(name: &str, rgba: &icon::IconRgba) {
    let img: ImageBuffer<Rgba<u8>, Vec<u8>> =
        ImageBuffer::from_raw(rgba.w, rgba.h, rgba.rgba.clone()).unwrap();
    let out = Path::new("target/icon_probe").join(name);
    img.save(&out).unwrap();
    let shape = match rgba.shape {
        icon::IconShape::Round => "ROUND",
        icon::IconShape::Square => "SQUARE",
    };
    println!(
        "saved {} ({}x{}, {shape})",
        out.display(),
        rgba.w,
        rgba.h
    );
}

fn main() {
    std::fs::create_dir_all("target/icon_probe").ok();

    // Baseline: raw exe icon (must have NO arrow).
    let notepad = Path::new(r"C:\Windows\System32\notepad.exe");
    let baseline = match icon::extract_icon(notepad) {
        Some(ic) => {
            save("baseline_notepad.png", &ic);
            ic
        }
        None => {
            println!("baseline notepad: FAILED");
            return;
        }
    };

    // End-to-end arrow check: a .lnk → notepad must yield byte-identical pixels
    // (the old direct .lnk path drew the overlay arrow in the bottom-left).
    let lnk = Path::new(r"target\icon_probe\probe_notepad.lnk");
    if lnk.exists() {
        match icon::extract_icon(lnk) {
            Some(ic) => {
                let same = ic == baseline;
                println!(
                    "lnk->notepad identical to exe: {same} ({}x{} vs {}x{})",
                    ic.w, ic.h, baseline.w, baseline.h
                );
                if !same {
                    println!("  bottom-left diff: {}", diff_bottom_left(&baseline, &ic));
                }
            }
            None => println!("lnk->notepad: FAILED"),
        }
    } else {
        println!("probe_notepad.lnk not found — create it with PowerShell first");
    }

    for (i, lnk) in find_lnks().iter().enumerate() {
        println!("--- .lnk #{}: {}", i, lnk.display());
        // 1. Direct SHGFI_ICON on the .lnk (old behavior — expect arrow).
        let direct = icon::extract_icon(lnk);
        match &direct {
            Some(ic) => save(&format!("lnk_{i}_direct.png"), ic),
            None => println!("  direct: FAILED"),
        }
        // 2. New resolution chain (icon location → IShellLinkW fallback).
        if let Some((src, idx)) = icon::icon_location(lnk) {
            println!("  iconlocation: {} (idx {idx}, exists {})", src.display(), src.exists());
        } else {
            println!("  iconlocation: NONE");
        }
        if let Some(tgt) = icon::lnk_target(lnk) {
            println!("  lnk target: {} (exists {})", tgt.display(), tgt.exists());
        } else {
            println!("  lnk target: NONE");
        }
        if let Some(ic) = icon::extract_icon(lnk) {
            save(&format!("lnk_{i}_resolved.png"), &ic);
            if let Some(d) = &direct {
                let diff = diff_bottom_left(d, &ic);
                println!(
                    "  bottom-left diff: {diff} px (arrow zone; 0 = same as before)"
                );
            }
        } else {
            println!("  resolved: FAILED");
        }
    }
}
